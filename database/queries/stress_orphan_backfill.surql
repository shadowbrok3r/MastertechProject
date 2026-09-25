-- One-time backfill of StressTestRun::reap_orphaned's window rule (2h grace). WRITES.
-- Dry run: execute the two LET statements and the RETURN only.

LET $note = '[orphaned: no finalize within planned window + 2h; ended_at is the last metric/event]';

-- In-progress runs past start + plan + 2h, each with its newest metric/event, kept when silent for 2h.
LET $orphans = array::filter(
    array::map(
        (SELECT id, hostname, preset_label, started_at, duration_planned_secs
            FROM stress_test_run
            WHERE result = 'in_progress'
                AND started_at + duration::from_secs(duration_planned_secs ?? 0) + 2h < time::now()),
        |$run| $run + {
            last_seen: time::max([
                $run.started_at,
                (SELECT VALUE captured_at FROM stress_test_metric
                    WHERE run_ref = $run.id ORDER BY captured_at DESC LIMIT 1)[0] ?? $run.started_at,
                (SELECT VALUE at FROM stress_test_event
                    WHERE run_ref = $run.id ORDER BY at DESC LIMIT 1)[0] ?? $run.started_at
            ])
        }
    ),
    |$run| $run.last_seen + 2h < time::now()
);

RETURN $orphans;

-- Closes each row still in progress as aborted/crashed, ended at its last heartbeat.
FOR $run IN $orphans {
    UPDATE $run.id SET
        result = 'aborted',
        finish_reason = 'crashed',
        ended_at = $run.last_seen,
        duration_actual_secs = <float> duration::secs($run.last_seen - started_at),
        notes = IF notes THEN string::concat(notes, ' ', $note) ELSE $note END
    WHERE result = 'in_progress';
};
