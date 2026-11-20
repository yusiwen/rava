//! S28 acceptance: reflection fields and methods are real objects; `Field.get/set` and
//! `Method.invoke` run through the JDK's own accessor machinery over the VM's `Unsafe`.
//!
//! Compared with `java` byte for byte.

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
class C {
    int x;
    static int S;
    C() { x = 5; }
    int twice(int v) { return v * 2; }
}

public class Members {
    public static void main(String[] args) throws Exception {
        C c = new C();
        java.lang.reflect.Field x = C.class.getDeclaredField("x");
        x.setAccessible(true);
        x.setInt(c, 9);
        System.out.println(x.getInt(c));

        java.lang.reflect.Field s = C.class.getDeclaredField("S");
        s.setAccessible(true);
        s.setInt(null, 7);
        System.out.println(s.getInt(null));

        java.lang.reflect.Method m = C.class.getDeclaredMethod("twice", int.class);
        m.setAccessible(true);
        System.out.println(m.invoke(c, 21));

        System.out.println(C.class.getDeclaredFields().length + " " + C.class.getDeclaredMethods().length);
    }
}
"#;

#[test]
fn fields_and_methods_match_the_reference_launcher() {
    let dir = std::env::temp_dir().join(format!("rava-members-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("fixture directory");
    let source = dir.join("Members.java");
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
        .arg("Members")
        .output()
        .expect("run java");
    let ours = Command::new(env!("CARGO_BIN_EXE_rava"))
        .arg("run")
        .arg("Members")
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
