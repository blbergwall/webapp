// Bridge between webapp-client (Rust/WASM) and the Discord Embedded App SDK.
// Bundled with the SDK into `discord.js` (see README.md). Edit this file, not
// `discord.js`.
import { DiscordSDK } from "@discord/embedded-app-sdk";

let sdk = null;

// Waits for the Discord client, then asks for an OAuth2 code with only the
// `identify` scope. Resolves to the code string; rejects if the user cancels
// or the SDK fails.
export async function discordAuthorize(clientId) {
  if (sdk === null) {
    sdk = new DiscordSDK(clientId);
    await sdk.ready();
  }
  const { code } = await sdk.commands.authorize({
    client_id: clientId,
    response_type: "code",
    state: "",
    prompt: "none",
    scope: ["identify"],
  });
  return code;
}

// Authenticates the SDK with the access token from the server's code
// exchange, for apps that use SDK commands beyond sign-in.
export async function discordAuthenticate(accessToken) {
  if (sdk === null) throw new Error("discordAuthorize must be called first");
  await sdk.commands.authenticate({ access_token: accessToken });
}
