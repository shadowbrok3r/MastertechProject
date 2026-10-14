# fleet-relay

Read-only HTTP relay on the shop box (.7, `192.168.22.7:7431`). It tells the admin-agent broker which fleet node last served each Codex thread, and which quant that node runs.

- Routes come from the highest-numbered `logs_<n>.sqlite` in Codex's `~/.config/zc-codexd/codex-home` (today `logs_2.sqlite`, opened `mode=ro`): the newest `codex_http_client::client` row per `thread_id` carrying `x-fleet-backend` / `x-fleet-route`. Compaction rows count.
- Quants come from probing each node every 60 s (llama.cpp `/props` + `/v1/models`, vLLM `/v1/models` + `/version`).
- Python 3 stdlib only. No coordinator token.

## API

| Request | Auth | Returns |
|---|---|---|
| `GET /health` | none | `{"ok":true}` |
| `GET /v1/routes?threads=<id>,<id>` (max 200, `[A-Za-z0-9-]{1,64}`) | `Authorization: Bearer <token>` | `{"checked_at","threads":[{"thread","node","route","served_at","quant","model"}]}` |
| `GET /v1/nodes` | same | `{"checked_at","nodes":[{"node","url","engine","build","model","quant","context","ok","probed_at"}]}` |

Threads with no header row are left out. `quant`/`model` are null when the node is unknown or its last probe failed.

## Config (env)

| Variable | Default |
|---|---|
| `FLEET_RELAY_BIND` | `192.168.22.7:7431` |
| `FLEET_RELAY_TOKEN_FILE` | `~/.config/fleet-relay/token` |
| `FLEET_RELAY_CODEX_HOME` | `~/.config/zc-codexd/codex-home` (reads its highest-numbered `logs_<n>.sqlite`) |
| `FLEET_RELAY_CODEX_LOG` | unset; a fixed log file that overrides `FLEET_RELAY_CODEX_HOME` |
| `FLEET_RELAY_NODES` | built-in table; else a JSON file `{"node-7900": "http://100.80.202.35:11436", ...}` |

## Install (on .7, as shadowbroker)

```sh
mkdir -p ~/fleet-relay ~/.config/fleet-relay && chmod 700 ~/.config/fleet-relay
cp fleet_relay.py test_fleet_relay.py ~/fleet-relay/
(umask 077; python3 -c 'import secrets; print(secrets.token_urlsafe(32))' > ~/.config/fleet-relay/token)
install -Dm644 fleet-relay.service ~/.config/systemd/user/fleet-relay.service
loginctl show-user "$USER" -p Linger   # must be Linger=yes
systemctl --user daemon-reload && systemctl --user enable --now fleet-relay
```

The broker reads the same token from k8s secret `fleet-relay` key `token` (`MTECH_FLEET_RELAY_TOKEN`) and the URL from `MTECH_FLEET_RELAY_URL=http://192.168.22.7:7431`.

## Verify

```sh
cd ~/fleet-relay && python3 -m unittest test_fleet_relay
curl -s http://192.168.22.7:7431/health
T=$(cat ~/.config/fleet-relay/token)
curl -s -H "Authorization: Bearer $T" http://192.168.22.7:7431/v1/nodes
curl -s -H "Authorization: Bearer $T" "http://192.168.22.7:7431/v1/routes?threads=<codex_thread_id>"
journalctl --user -u fleet-relay -n 20
```
