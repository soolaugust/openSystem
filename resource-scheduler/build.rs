fn main() {
    #[cfg(feature = "ebpf")]
    {
        use libbpf_cargo::SkeletonBuilder;
        use std::path::Path;

        let bpf_src = Path::new("src/bpf/sched_monitor.bpf.c");

        // Only build if the BPF source exists (allows the feature to be
        // enabled before BPF programs are written).
        if bpf_src.exists() {
            let skel_out = Path::new("src/bpf/sched_monitor.skel.rs");
            SkeletonBuilder::new()
                .source(bpf_src)
                .build_and_generate(skel_out)
                .expect("failed to build and generate BPF skeleton");
            println!("cargo:rerun-if-changed={}", bpf_src.display());
        } else {
            println!(
                "cargo:warning=eBPF feature enabled but {} not found, skipping BPF build",
                bpf_src.display()
            );
        }
    }
}
