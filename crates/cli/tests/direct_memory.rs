//! S29a-1 acceptance: `ByteBuffer.allocateDirect` over the VM's off-heap memory.
//!
//! The JDK's `DirectByteBuffer` calls `Unsafe.allocateMemory`/`setMemory` and reads/writes
//! through `Unsafe` with a **null base** (an `Address`); this VM keeps such memory in a map and
//! serves the null-base accessors from it. Compared with `java` byte for byte.

use std::path::PathBuf;
use std::process::Command;

fn jdk_home() -> PathBuf {
    let home = std::env::var_os("JAVA21_HOME").unwrap_or_else(|| {
        panic!(
            "JAVA21_HOME is not set: run the tests inside `nix develop` (the devShell exports it)"
        )
    });
    PathBuf::from(home)
}

const SOURCE: &str = r#"
import java.nio.ByteBuffer;

public class Direct {
    public static void main(String[] args) {
        ByteBuffer b = ByteBuffer.allocateDirect(8);
        b.putInt(0, 42);
        b.putInt(4, -7);
        System.out.println(b.getInt(0));
        System.out.println(b.getInt(4));
        System.out.println(b.capacity());
        b.put(0, (byte) 9);
        System.out.println(b.get(0));

        ByteBuffer big = ByteBuffer.allocateDirect(16);
        big.putLong(0, 1234567890123L);
        System.out.println(big.getLong(0));
        big.putDouble(8, 2.5);
        System.out.println(big.getDouble(8));
    }
}
"#;

#[test]
fn direct_buffers_match_the_reference_launcher() {
    let dir = std::env::temp_dir().join(format!("rava-direct-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("fixture directory");
    let source = dir.join("Direct.java");
    std::fs::write(&source, SOURCE).expect("source file");
    let compiled = Command::new(jdk_home().join("bin/javac"))
        .arg("-d")
        .arg(&dir)
        .arg(&source)
        .output()
        .expect("run javac");
    assert!(
        compiled.status.success(),
        "javac failed: {}",
        String::from_utf8_lossy(&compiled.stderr)
    );

    let reference = Command::new(jdk_home().join("bin/java"))
        .arg("-cp")
        .arg(&dir)
        .arg("Direct")
        .output()
        .expect("run java");
    let ours = Command::new(env!("CARGO_BIN_EXE_rava"))
        .arg("run")
        .arg("Direct")
        .arg("--home")
        .arg(jdk_home())
        .arg("--cp")
        .arg(&dir)
        .output()
        .expect("run rava");

    assert_eq!(
        ours.status.code(),
        reference.status.code(),
        "exit code: rava {:?}, java {:?}\nstderr: {}",
        ours.status.code(),
        reference.status.code(),
        String::from_utf8_lossy(&ours.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&ours.stdout),
        String::from_utf8_lossy(&reference.stdout),
        "stdout differs\nrava: {}\njava: {}",
        String::from_utf8_lossy(&ours.stdout),
        String::from_utf8_lossy(&reference.stdout)
    );
    assert!(!reference.stdout.is_empty());
}
