//! S25 acceptance: lambda dispatch is SAM-only, plus the enablers the regex/Formatter path needed.
//!
//! A lambda object implements one interface method (its SAM). Any *other* call on it — an
//! interface default such as `CharPredicate.union` — must run the default method, not the lambda
//! body. Treating every call as the SAM fed the lambda's parameter list a foreign argument list
//! and surfaced as `expected int, found Ref` inside `java.util.regex.Pattern` (issue #37).
//!
//! The same program pins three enablers this slice added: a `synchronized static` method (the
//! Class-object monitor), `String.intern`, and `Class.forName` with initialization.

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
    let dir = std::env::temp_dir().join(format!("rava-lambda-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("fixture directory");
    dir
}

const SOURCE: &str = r#"
interface F {
    int apply(int x);
    default int twice(int x) { return apply(apply(x)); }
}

class B {
    static int X;
    static {
        System.out.println("B init");
        X = 9;
    }
}

public class Lambdas {
    static synchronized int locked() { return 7; }

    public static void main(String[] args) throws Exception {
        F f = x -> x + 1;
        System.out.println(f.apply(1));
        System.out.println(f.twice(1));
        System.out.println(locked());
        System.out.println("abc" == new String("abc").intern());
        System.out.println(Class.forName("java.lang.String") == String.class);
        Class.forName("B");
        System.out.println(B.X);
    }
}
"#;

#[test]
fn lambda_sam_and_the_enablers_match_the_reference_launcher() {
    let dir = fixture_dir();
    let source = dir.join("Lambdas.java");
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
        .arg("Lambdas")
        .output()
        .expect("run java");
    let ours = Command::new(env!("CARGO_BIN_EXE_rava"))
        .arg("run")
        .arg("Lambdas")
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
