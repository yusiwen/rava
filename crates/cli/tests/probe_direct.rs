use std::path::Path;

use rava_vm::{ClassPath, Shared};

#[test]
fn probe() {
    let home = std::env::var("JAVA21_HOME").unwrap();
    let path: ClassPath = rava_cli::class_path(Path::new(&home), Path::new(".")).unwrap();
    let mut shared = Shared::new();
    rava_native::register_jcl_natives(&mut shared);
    let unsafe_class = shared
        .load_class(&path, "jdk/internal/misc/Unsafe")
        .unwrap();
    for (name, desc) in [
        ("allocateMemory0", "(J)J"),
        ("setMemory0", "(Ljava/lang/Object;JJB)V"),
        ("freeMemory0", "(J)V"),
    ] {
        let index = shared.class(unsafe_class).method_index(name, desc);
        println!(
            "{name}{desc} index={index:?} native={:?} key={}",
            index.map(|i| shared.class(unsafe_class).methods[i].native.is_some()),
            shared.natives.contains_key(&rava_vm::native::key(
                "jdk/internal/misc/Unsafe",
                name,
                desc
            ))
        );
    }
}
