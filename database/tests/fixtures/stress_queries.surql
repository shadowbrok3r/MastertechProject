-- Executable fixtures for `surreal validate`. Uses LET bindings so statements are complete.

LET $id = hardware_component:84e216e0b2fe67801ccc83a6bee2a4a8;
LET $kind = "cpu";
LET $vendor = "AMD";
LET $model = "AMD Ryzen 5 5600G with Radeon Graphics";
LET $sku = NONE;
LET $display = "AMD AMD Ryzen 5 5600G with Radeon Graphics";
LET $specs = NONE;
LET $embed_src = "cpu AMD AMD Ryzen 5 5600G with Radeon Graphics AMD AMD Ryzen 5 5600G with Radeon Graphics";

UPSERT $id MERGE {
    kind: $kind,
    vendor: $vendor,
    model: $model,
    sku: $sku,
    display_name: $display,
    specs: $specs,
    embedding: fn::embed_text($embed_src),
    first_seen: time::now(),
    last_seen: time::now(),
    occurrence_count: (occurrence_count ?? 0) + 1,
} RETURN id;

LET $run_id = stress_test_run:`5679c02e-48e3-40ff-99f6-8e0c8a01f9dc`;
LET $computer = computer:`DESKTOP-3F0BA5T:f4ac11309`;
CREATE $computer CONTENT { hostname: "DESKTOP-3F0BA5T" };

LET $content = {
    computer: $computer,
    target_kind: "cpu",
    tool: { StressKit: { stressor: "cpu" } },
    tool_label: "stresskit:cpu",
    failure_mode: { None: {} },
    failure_kind: "none",
    result: "in_progress",
    summary: {},
    started_at: time::now(),
};
LET $embed_src = "stresskit:cpu scripts:single:cpu cpu DESKTOP-3F0BA5T";

CREATE $run_id CONTENT ($content + { embedding: fn::embed_text($embed_src) });

RETURN record::exists($run_id);
