{
  cargo-deny,
  clippy,
  lib,
  python3,
  rustPlatform,
  rustfmt,
  stdenv,
}:

rustPlatform.buildRustPackage (finalAttrs: {
  pname = "research-provenance";
  version = "0.1.0";

  # Standalone repository: the package source is the repo root itself, with the
  # conformance fixtures and JSON schemas as first-class top-level directories.
  src = lib.fileset.toSource {
    root = ./.;
    fileset = lib.fileset.unions [
      ./Cargo.toml
      ./Cargo.lock
      ./crates
      ./resources
      ./fixtures
      ./schemas
      ./tools/pilot-reader
      ./tools/test-release
    ];
  };

  cargoRoot = ".";
  buildAndTestSubdir = finalAttrs.cargoRoot;

  cargoLock = {
    lockFile = ./Cargo.lock;
  };

  cargoBuildFlags = [
    "--package"
    "rp-cli"
    "--bin"
    "rp"
    "--package"
    "rp-server"
    "--bin"
    "rp-server"
  ];

  checkType = "debug";
  cargoTestFlags = [ "--workspace" ];

  nativeCheckInputs = [
    cargo-deny
    clippy
    rustfmt
  ];

  preCheck = ''
    cargo fmt --manifest-path ${finalAttrs.cargoRoot}/Cargo.toml --all --check
    cargo clippy \
      --manifest-path ${finalAttrs.cargoRoot}/Cargo.toml \
      --locked \
      --offline \
      --target ${stdenv.hostPlatform.rust.rustcTarget} \
      --workspace \
      --all-targets \
      -- \
      -D warnings
    cargo deny \
      --manifest-path ${finalAttrs.cargoRoot}/crates/rp-cli/Cargo.toml \
      --exclude-dev \
      --offline \
      --locked \
      check \
      --config "$PWD/${finalAttrs.cargoRoot}/deny.toml" \
      bans sources
  '';

  postInstall = ''
    reader="$out/share/research-provenance/reader"
    mkdir -p "$reader"
    for name in build.py lookup.py case_io.py source_locators.py graph_projection.py routing.py \
      routing.js template.html reader.css graph.js bootstrap.js svg.js; do
      install -m 0644 "${finalAttrs.src}/tools/pilot-reader/$name" "$reader/$name"
    done
    for name in rp-view rp-lookup; do
      substitute "${finalAttrs.src}/tools/pilot-reader/installed_launcher.py" \
        "$out/bin/$name" --subst-var-by python '${python3}/bin/python3'
      chmod 0755 "$out/bin/$name"
    done
  '';

  doInstallCheck = true;
  installCheckPhase = ''
    runHook preInstallCheck
    test -x "$out/bin/rp"
    test "$(find "$out/bin" -maxdepth 1 -type f -executable -printf '%f\n' | sort)" = "$(printf 'rp\nrp-lookup\nrp-server\nrp-view')"
    test ! -e "$out/lib"
    "$out/bin/rp" --help >/dev/null
    "$out/bin/rp" --version | grep -Fx 'rp 0.1.0' >/dev/null
    scratch=$(mktemp -d)
    cp tools/pilot-reader/tests/installed_reader.py "$scratch/installed_reader.py"
    (cd "$scratch"; ${python3}/bin/python3 -I installed_reader.py --package "$out" -v)
    runHook postInstallCheck
  '';

  meta = {
    description = "Offline, deterministic research provenance validation and navigation CLI";
    license = lib.licenses.mit;
    mainProgram = "rp";
    platforms = lib.platforms.linux;
  };
})
