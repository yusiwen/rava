//! S27 acceptance: reflection constructors are real objects and `newInstance` runs them.
//!
//! `Class.getDeclaredConstructors()` returns `Constructor` instances the host's own methods can
//! read (`getDeclaringClass`, `getParameterCount`), and `Constructor.newInstance` allocates the
//! class and invokes `<init>` — including a boxed primitive argument. Compared with `java`.

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

fn fixture_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rava-reflect-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("fixture directory");
    dir
}

const SOURCE: &str = r#"
class C {
    int x;
    C() { x = 5; }
}

class D {
    int y;
    D(int v) { y = v; }
}

public class Reflection {
    public static void main(String[] args) throws Exception {
        java.lang.reflect.Constructor<?>[] all = C.class.getDeclaredConstructors();
        System.out.println(all.length);
        java.lang.reflect.Constructor<?> c = all[0];
        System.out.println(c.getDeclaringClass().getName());
        System.out.println(c.getParameterCount());
        C instance = (C) c.newInstance();
        System.out.println(instance.x);

        java.lang.reflect.Constructor<?> d = D.class.getDeclaredConstructors()[0];
        D boxed = (D) d.newInstance(7);
        System.out.println(boxed.y);
    }
}
"#;

#[test]
fn declared_constructors_and_new_instance_match_the_reference_launcher() {
    let dir = fixture_dir();
    let source = dir.join("Reflection.java");
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
        .arg("Reflection")
        .output()
        .expect("run java");
    let ours = Command::new(env!("CARGO_BIN_EXE_rava"))
        .arg("run")
        .arg("Reflection")
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
    assert!(
        !reference.stdout.is_empty(),
        "the reference output is empty, so the comparison proves nothing"
    );
}
