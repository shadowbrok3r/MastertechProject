use database::schema::{
    BenchmarkKind, BenchmarkResult, EventKind, RecordId, StressTestEvent, StressTestRun,
    TargetKind, TestTool, UNREGISTERED_BUILD,
};

#[test]
fn set_host_build_publishes_the_composed_stamp_to_every_writer() {
    assert_eq!(stress_runner::host_build(), UNREGISTERED_BUILD);
    assert_eq!(stress_runner::tool_version(), UNREGISTERED_BUILD);
    assert_eq!(
        stress_runner::recorded_tool_version(None),
        format!("backfill; recorded by {UNREGISTERED_BUILD}")
    );

    stress_runner::set_host_build("TestApp", "0.0.0+0000000");
    stress_runner::set_host_build("OtherApp", "1.1.1+1111111");

    let expected = format!("TestApp 0.0.0+0000000; stress-kit {}", stress_kit::BUILD_ID);
    assert_eq!(database::schema::process_build(), expected);
    assert_eq!(stress_runner::tool_version(), expected);
    assert_eq!(stress_runner::host_build(), "TestApp 0.0.0+0000000");
    assert_eq!(
        stress_runner::recorded_tool_version(None),
        "backfill; recorded by TestApp 0.0.0+0000000"
    );

    let computer = RecordId::new("computer", "DESKTOP-TEST:0000000");
    let run = StressTestRun::new_for(
        computer.clone(),
        TestTool::StressKit {
            stressor: "cpu".into(),
        },
        TargetKind::Cpu,
    )
    .stamped();
    assert_eq!(run.tool_version.as_deref(), Some(expected.as_str()));

    let event = StressTestEvent::new(run.id.clone(), EventKind::Tdr, "telemetry").stamped();
    assert_eq!(event.tool_version.as_deref(), Some(expected.as_str()));

    let bench = BenchmarkResult::new(computer, BenchmarkKind::MemoryLatency, 80.0, "ns").stamped();
    assert_eq!(bench.run_ref, None);
    assert_eq!(bench.tool_version.as_deref(), Some(expected.as_str()));
}
