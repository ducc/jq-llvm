{
  description = "jq-llvm: compile jq programs to native executables via LLVM";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
        clang = pkgs.llvmPackages_19.clang;

        jq-llvm = pkgs.rustPlatform.buildRustPackage {
          pname = "jq-llvm";
          version = "0.1.0";
          src = pkgs.lib.cleanSource ./.;
          cargoLock.lockFile = ./Cargo.lock;
          nativeBuildInputs = [ pkgs.makeWrapper ];
          # the compiler shells out to clang to turn its LLVM IR into an executable
          postInstall = ''
            wrapProgram $out/bin/jq-llvm --set-default JQC_CLANG ${clang}/bin/clang
          '';
          meta = {
            description = "Compile jq programs to native executables via LLVM";
            mainProgram = "jq-llvm";
            license = pkgs.lib.licenses.mit;
          };
        };
      in
      {
        packages.default = jq-llvm;
        packages.jq-llvm = jq-llvm;

        apps.default = flake-utils.lib.mkApp { drv = jq-llvm; };

        # differential tests against the reference jq (`nix flake check`)
        checks.difftest = pkgs.runCommand "jq-llvm-difftest"
          { nativeBuildInputs = [ jq-llvm pkgs.jq clang pkgs.bash pkgs.coreutils pkgs.findutils pkgs.gnugrep pkgs.gawk ]; }
          ''
            cp -r ${pkgs.lib.cleanSource ./.}/tests tests
            chmod -R u+w tests
            export JQC=${jq-llvm}/bin/jq-llvm JOBS=$NIX_BUILD_CORES
            patchShebangs tests
            out_log=$(tests/difftest.sh tests/cases/basic.txt)
            echo "$out_log" | tail -20
            if echo "$out_log" | grep -q 'MISMATCH\|COMPILE-FAIL'; then exit 1; fi
            touch $out
          '';

        devShells.default = pkgs.mkShell {
          packages = [
            pkgs.cargo
            pkgs.rustc
            pkgs.rustfmt
            pkgs.clippy
            pkgs.rust-analyzer
            clang
            pkgs.llvmPackages_19.llvm
            pkgs.jq
            pkgs.time
          ] ++ pkgs.lib.optionals pkgs.stdenv.isLinux [ pkgs.valgrind pkgs.linuxPackages.perf ];
          JQC_CLANG = "${clang}/bin/clang";
        };
      });
}
