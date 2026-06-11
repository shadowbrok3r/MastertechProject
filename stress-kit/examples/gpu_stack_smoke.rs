fn main() {
    let report = stress_kit::gpu_stack::check_gpu_stack_uncached();
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
}
