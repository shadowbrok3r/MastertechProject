//! Runs admin commands against this machine, in-process.
//!
//! This is [`crate::tcp_listener::serve_admin_session`] with the handshake, the
//! writer task and the reader task removed: there is no socket, so there is
//! nothing to authenticate, frame or read. What remains — a per-session
//! [`TerminalWebsocketClient`], a [`ClientTransport`], and the shared
//! [`dispatch_task`] FIFO — is identical, which is why every viewer and the
//! whole receive path work over this transport unchanged.
//!
//! `handle_command` can block for minutes (registry walks, WMI, evtx, installs),
//! so it runs on the dispatch task exactly as it does for TCP. The UI thread only
//! ever pushes to an unbounded channel and pops from a mutex-guarded queue.

use displays::tabs::admin_console::client_interface::LocalPeer;
use ewebsock::{WsEvent, WsMessage};
use tokio::sync::mpsc::unbounded_channel;
use tokio::task::JoinHandle;

use crate::tcp_listener::{FILE_CHANNEL_DEPTH, SessionWork, dispatch_task};
use crate::terminal_mode::websockets::TerminalWebsocketClient;
use crate::transport::{ClientTransport, TcpFrame};

/// Aborts the pump when dropped, so closing the session cannot leave it running.
pub struct LocalSessionHandle(JoinHandle<()>);

impl Drop for LocalSessionHandle {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Starts the pump for `peer` and returns its handle.
pub fn spawn(peer: LocalPeer) -> LocalSessionHandle {
    LocalSessionHandle(tokio::spawn(run(peer)))
}

async fn run(mut peer: LocalPeer) {
    let (file_tx, mut file_rx) = tokio::sync::mpsc::channel::<TcpFrame>(FILE_CHANNEL_DEPTH);

    let mut client = TerminalWebsocketClient::new();
    // Deliberately NOT `arm_terminal_stream()`. That counter is process-global:
    // arming it here would make the ratatui render loop stream buffers for the
    // life of the app, to a session that has no Terminal page to show them.
    let outbound = client.take_outbound_receivers();
    let mut command_rx = outbound.command_rx;
    let mut sysinfo_rx = outbound.sysinfo_rx;
    let mut bin_rx = outbound.bin_rx;

    let transport = ClientTransport::Local {
        ctrl: peer.in_tx.clone(),
        file: file_tx.clone(),
    };
    let (work_tx, work_rx) = unbounded_channel::<SessionWork>();
    let mut dispatch = tokio::spawn(dispatch_task(client, transport, work_rx));

    log::info!("local_session -> started");

    loop {
        tokio::select! {
            biased;
            _ = displays::wait_for_shutdown() => break,
            frame = peer.out_rx.recv() => match frame {
                None => break,
                Some(displays::tabs::admin_console::client_interface::TcpFrame::Shutdown) => break,
                Some(displays::tabs::admin_console::client_interface::TcpFrame::Binary(bytes)) => {
                    match displays::try_deserialize_command(&bytes) {
                        Some(cmd) => {
                            if work_tx.send(SessionWork::Command(cmd)).is_err() {
                                break;
                            }
                        }
                        None => log::warn!(
                            "local_session -> undecodable Cmd ({} bytes)",
                            bytes.len()
                        ),
                    }
                }
                Some(displays::tabs::admin_console::client_interface::TcpFrame::Text(text)) => {
                    if work_tx.send(SessionWork::Text(text)).is_err() {
                        break;
                    }
                }
            },
            // The side channels the dispatcher streams responses on, so live data
            // reaches the UI without another request driving the loop.
            Some(bin) = command_rx.recv() => {
                let _ = peer.in_tx.send(WsEvent::Message(WsMessage::Binary(bin)));
            }
            Some(sysinfo) = sysinfo_rx.recv() => {
                let _ = peer.in_tx.send(WsEvent::Message(WsMessage::Binary(sysinfo)));
            }
            Some(bin) = bin_rx.recv() => {
                let _ = peer.in_tx.send(WsEvent::Message(WsMessage::Binary(bin)));
            }
            // Bounded file chunks, in place of the writer task's file branch.
            Some(chunk) = file_rx.recv() => {
                if let TcpFrame::Binary(bytes) = chunk {
                    let _ = peer.in_tx.send(WsEvent::Message(WsMessage::Binary(bytes)));
                }
            }
        }
    }

    // Drain queued work before tearing down, matching the TCP path; a process
    // shutdown cancels it rather than waiting.
    drop(work_tx);
    let aborted = tokio::select! {
        biased;
        _ = displays::wait_for_shutdown() => {
            dispatch.abort();
            true
        }
        _ = &mut dispatch => false,
    };
    if aborted {
        let _ = dispatch.await;
    }

    let _ = peer.in_tx.send(WsEvent::Closed);
    log::info!("local_session -> stopped");
}

#[cfg(test)]
mod local_session_tests {
    use super::*;
    use displays::Cmd;
    use displays::tabs::admin_console::client_interface::AdminTransport;
    use std::time::{Duration, Instant};

    /// Polls the admin side for up to `budget`, returning the first decoded `Cmd`.
    async fn wait_for_cmd(transport: &mut AdminTransport, budget: Duration) -> Option<Cmd> {
        let deadline = Instant::now() + budget;
        while Instant::now() < deadline {
            while let Some(ev) = transport.try_recv() {
                if let WsEvent::Message(WsMessage::Binary(bytes)) = ev {
                    if let Some(cmd) = displays::try_deserialize_command(&bytes) {
                        return Some(cmd);
                    }
                }
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        None
    }

    /// The whole point of the slice: a command reaches this machine and its reply
    /// comes back, with no socket, no handshake and no second session row.
    /// AppPing is used because its handler touches no OS state at all.
    #[tokio::test]
    async fn a_command_round_trips_with_no_socket() {
        let (mut transport, peer) = AdminTransport::from_local("test-roundtrip");
        let _pump = spawn(peer);

        transport.send_cmd(&Cmd::AppPing {
            nonce: 0xFEED,
            sent_at_ms: 1234,
        });

        match wait_for_cmd(&mut transport, Duration::from_secs(10)).await {
            Some(Cmd::AppPong { nonce, sent_at_ms }) => {
                assert_eq!(nonce, 0xFEED);
                assert_eq!(sent_at_ms, 1234);
            }
            other => panic!("expected AppPong, got {other:?}"),
        }
    }

    /// A malformed frame must be dropped, not take the session down with it.
    #[tokio::test]
    async fn an_undecodable_frame_does_not_kill_the_session() {
        let (mut transport, peer) = AdminTransport::from_local("test-garbage");
        let _pump = spawn(peer);

        transport.send(WsMessage::Binary(vec![0xFF; 32]));
        transport.send_cmd(&Cmd::AppPing {
            nonce: 1,
            sent_at_ms: 2,
        });

        assert!(
            matches!(
                wait_for_cmd(&mut transport, Duration::from_secs(10)).await,
                Some(Cmd::AppPong { nonce: 1, .. })
            ),
            "the session must survive a frame it cannot decode"
        );
    }

    /// Closing the tab must stop the pump rather than leaving it running for the
    /// life of the process.
    #[tokio::test]
    async fn closing_the_transport_stops_the_pump() {
        let (mut transport, peer) = AdminTransport::from_local("test-close");
        let pump = spawn(peer);

        transport.send_cmd(&Cmd::AppPing {
            nonce: 5,
            sent_at_ms: 6,
        });
        assert!(
            wait_for_cmd(&mut transport, Duration::from_secs(10))
                .await
                .is_some()
        );

        transport.close();
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(pump.0.is_finished(), "the pump must exit on Shutdown");
    }
}
