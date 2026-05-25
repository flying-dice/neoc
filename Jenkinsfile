// Jenkins pipeline for neoc.
//
// Runs on the Windows controller (the only available agent). This mirrors
// the Linux job in `.github/workflows/ci.yml` — fmt, clippy, Rust unit tests,
// Luau integration tests, and a release build — but executes them against the
// `x86_64-pc-windows-msvc` target.
//
// Coverage gap: macOS + Linux matrix entries from GHA are not reproduced here.
// Add agents and a parallel matrix block when those platforms are available.

pipeline {
    agent any

    options {
        timeout(time: 60, unit: 'MINUTES')
        buildDiscarder(logRotator(numToKeepStr: '20', artifactNumToKeepStr: '5'))
        disableConcurrentBuilds()
    }

    environment {
        CARGO_TERM_COLOR = 'always'
        RUSTFLAGS        = '-D warnings'
        RUSTUP_HOME      = "${env.WORKSPACE}\\.rustup"
        CARGO_HOME       = "${env.WORKSPACE}\\.cargo"
        PATH             = "${env.WORKSPACE}\\.cargo\\bin;${env.PATH}"
    }

    stages {
        stage('Toolchain') {
            steps {
                bat '''
                    if not exist "%CARGO_HOME%\\bin\\cargo.exe" (
                        echo Installing rustup toolchain...
                        powershell -NoProfile -Command "Invoke-WebRequest https://win.rustup.rs/x86_64 -OutFile rustup-init.exe"
                        rustup-init.exe -y --default-toolchain stable --profile minimal --component clippy --component rustfmt
                        del rustup-init.exe
                    ) else (
                        rustup update stable
                    )
                    rustc --version
                    cargo --version
                    cargo clippy --version
                    cargo fmt --version
                '''
            }
        }

        stage('Check & Lint') {
            steps {
                bat 'cargo fmt --check'
                bat 'cargo clippy --all-targets'
            }
        }

        stage('Test') {
            steps {
                bat 'cargo test --workspace --lib --target x86_64-pc-windows-msvc'
                bat 'if not exist .tmp mkdir .tmp'
                bat '''
                    cargo run -p neoc --target x86_64-pc-windows-msvc -- run ^
                        "crates/neoc/tests/lib/*.test.luau" ^
                        "crates/neoc/tests/std/*.test.luau" ^
                        "crates/neoc/tests/vnd/jsonschema.test.luau" ^
                        "crates/neoc/tests/vnd/quick_xml.test.luau" ^
                        "crates/neoc/tests/vnd/rusqlite.test.luau" ^
                        "crates/neoc/tests/vnd/serde_json.test.luau" ^
                        "crates/neoc/tests/vnd/serde_yaml.test.luau" ^
                        "crates/neoc/tests/vnd/toml.test.luau"
                '''
            }
        }

        stage('Build') {
            steps {
                bat 'cargo build --release --target x86_64-pc-windows-msvc'
            }
            post {
                success {
                    archiveArtifacts(
                        artifacts: 'target/x86_64-pc-windows-msvc/release/neoc.exe',
                        fingerprint: true
                    )
                }
            }
        }
    }

    post {
        always {
            // Keep cargo target/ between runs for incremental compile speed,
            // but drop transient artefacts so the workspace doesn't grow forever.
            bat 'if exist .tmp rmdir /s /q .tmp'
        }
    }
}
