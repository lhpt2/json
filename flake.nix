{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = {
    self,
    nixpkgs,
    rust-overlay,
  }: let
    system = "x86_64-linux";
    pkgs = import nixpkgs {
      inherit system;
      overlays = [rust-overlay.overlays.default];
    };
    toolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
  in {
    devShells.${system}.default = pkgs.mkShell {
	
	    nativeBuildInputs = with pkgs; [
			libclang
    			pkg-config
			cmake
  		] ++ [
		  lldb
  		];
		
      packages = [
        toolchain
	pkgs.clang
	pkgs.rust-analyzer-unwrapped
      ];
		
      #LD_LIBRARY_PATH = "/run/opengl-driver/lib/:${pkgs.lib.makeLibraryPath([pkgs.libGL pkgs.libGLU])}";
      LIBCLANG_PATH = "${pkgs.lib.makeLibraryPath([ pkgs.llvmPackages_latest.libclang.lib ])}";
      RUST_SRC_PATH = "${toolchain}/lib/rustlib/src/rust/library";
    };
  };
}
