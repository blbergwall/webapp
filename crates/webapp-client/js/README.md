# Discord SDK bridge

`discord.js` is **generated**: `bridge.src.js` bundled with
`@discord/embedded-app-sdk@2.5.0` into one minified ES module, with no external
imports (Discord's activity CSP blocks unmapped URLs, so nothing can come from
a CDN). It's loaded as a wasm-bindgen snippet by `src/discord.rs`, so apps get it
automatically through the crate dependency.

Node/esbuild are only needed to regenerate it, not to build. To upgrade the SDK
or change the bridge, edit `bridge.src.js`, then in an empty scratch directory:

```sh
npm init -y
npm install @discord/embedded-app-sdk@<version> esbuild
cp <repo>/crates/webapp-client/js/bridge.src.js .
npx esbuild bridge.src.js --bundle --format=esm --minify --target=es2020 --legal-comments=none --outfile=out.js
```

Prepend the 3-line `// GENERATED` header (update the version), write the
result to `discord.js`, and update the version here and in `CLAUDE.md`.

`THIRD-PARTY-NOTICES` holds the license texts for the SDK and the libraries it
ships pre-bundled (`node_modules/@discord/embedded-app-sdk/output/lib/`). The
sources have no legal comments, so esbuild can't collect them. When upgrading
the SDK, compare that directory's library list with the notices file and add or
remove entries, copying each package's own license file.
