# packages.neo-web-css — Tailwind + daisyUI → $out/neo-ui.css (used by packages.neo STATIC_DIR).
#
# Tailwind's @source paths are relative to cli/web-css/input.css, so the
# derivation src keeps that layout and drops every other file under cli/.
{
  lib,
  self,
  ...
}: {
  perSystem = {pkgs, ...}: let
    slice = name: root: keepDir: keepFile:
      lib.cleanSourceWith {
        inherit name;
        src = root;
        filter = path: type: let
          rootStr = toString root;
          rel = lib.removePrefix "${rootStr}/" (toString path);
          base = baseNameOf path;
        in
          if toString path == rootStr
          then true
          else if type == "directory"
          then keepDir rel
          else keepFile rel base;
      };

    cliRoot = self + "/cli";
    npmSrc = slice "neo-web-css-npm-src" (cliRoot + "/web-css") (_rel: false) (
      _rel: base: base == "package.json" || base == "package-lock.json"
    );
    cssSrc =
      slice "neo-web-css-src" cliRoot (
        rel:
          rel
          == "web-css"
          || rel == "templates"
          || rel == "static"
          || rel == "src"
          || lib.hasPrefix "templates/" rel
          || lib.hasPrefix "static/" rel
          || lib.hasPrefix "src/" rel
      ) (
        rel: base:
          rel
          == "web-css/input.css"
          || (lib.hasPrefix "templates/" rel && lib.hasSuffix ".hbs" base)
          || (
            lib.hasPrefix "static/" rel
            && base != "neo-ui.css"
            && (lib.hasSuffix ".js" base || lib.hasSuffix ".css" base || lib.hasSuffix ".html" base)
          )
          || (lib.hasPrefix "src/" rel && lib.hasSuffix ".rs" base)
      );

    npm = pkgs.buildNpmPackage {
      pname = "neo-web-css-npm";
      version = "0.1.0";
      src = npmSrc;
      npmDepsHash = "sha256-/RHHcncYWVnxM29vWNbsEBZ+pjaRTN96G6BhyATbQ7M=";
      dontNpmBuild = true;
      installPhase = ''
        runHook preInstall
        mkdir -p $out
        cp -a node_modules $out/
        cp package.json $out/
        runHook postInstall
      '';
    };

    neoWebCss = pkgs.stdenvNoCC.mkDerivation {
      pname = "neo-web-css";
      version = "0.1.0";
      src = cssSrc;
      nativeBuildInputs = [pkgs.nodejs];
      buildPhase = ''
        runHook preBuild
        rm -rf web-css/node_modules
        ln -s ${npm}/node_modules web-css/node_modules
        ./web-css/node_modules/.bin/tailwindcss \
          -i ./web-css/input.css \
          -o ./neo-ui.css \
          --minify
        runHook postBuild
      '';
      installPhase = ''
        runHook preInstall
        mkdir -p $out
        cp neo-ui.css $out/neo-ui.css
        runHook postInstall
      '';
    };
  in {
    packages.neo-web-css = neoWebCss;
  };
}
