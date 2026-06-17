{
    description = "Development environment for Etash: Even That's A SHell";

    inputs = {
        nixpkgs.url = "github:Nixos/nixpkgs/nixos-unstable";
    };

    outputs = { self, nixpkgs }:
    let
      supportedSystems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];

      forAllSystems = nixpkgs.lib.genAttrs supportedSystems;
    in
    {
      devShells = forAllSystems (system:
        let
          pkgs = import nixpkgs { inherit system; };
        in
        {
          default = pkgs.mkShell {
            packages = with pkgs; [
              git

              cargo
            ];

            shellHook = ''
              echo "Etash development environment activated"
            '';
          };
        });
    };
}
