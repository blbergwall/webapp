# Discord Activity research

**Status:** done

I need to understand more about what Discord activities are and how they work. I am using them in this framework mainly as an identity provider, and they seem to work well for that, but I feel there is a lot more to them.

In the configuration in the Discord Developer Portal, there is a setting under Activities, "Maximum Participants", with the subheading "The maximum participants allowed in your activity". What does this mean? I don't think it means the maximum number of people who can use your activity across all of Discord. I am guessing it is the maximum number of users who can join the same "instance" of an activity that is started in a server channel? But what does that mean, and what gets shared between users joining the same instance?

What other things are fundamentally different between an activity and a plain website running in a browser? Can activities only run single-page apps?

## Scope

- **Decision:** Answer the three questions (what an activity is beyond identity, what "Maximum Participants" and an "instance" mean and what is shared, and how an activity differs from a plain website, including whether it must be a SPA). Note anything that matters for this framework, but make no code changes.

## Findings

### 1. What an activity is

An activity is a web app that Discord loads in an **iframe** inside the Discord client (desktop, web, iOS, Android). The page talks to the Discord client through `postMessage`, wrapped by the Embedded App SDK. Discord's own framing is "multiplayer games and social experiences": the point of an activity is that **a group of people in the same channel or call use it together**. Identity is only the first step.

Beyond `authorize`/`authenticate`, the SDK gives the app (full list in the [SDK reference](https://docs.discord.com/developers/developer-tools/embedded-app-sdk)):

- **Who's here:** `getActivityInstanceConnectedParticipants` and the `ACTIVITY_INSTANCE_PARTICIPANTS_UPDATE` event (no extra scopes).
- **Voice context:** `VOICE_STATE_UPDATE`, `SPEAKING_START`/`SPEAKING_STOP` (scope `rpc.voice.read`), e.g. to highlight whoever is talking.
- **Channel/guild context:** `getChannel`, `getChannelPermissions`, `CURRENT_GUILD_MEMBER_UPDATE` (guild nickname/avatar).
- **Growth:** `openInviteDialog` (invite others to the channel; errors in DMs), `shareLink` (share a link that launches the activity, with an optional `custom_id` that comes back as a launch parameter), `openShareMomentDialog` (post an image to a channel/DM).
- **Presence:** `setActivity` sets the user's Rich Presence (e.g. "Playing X, round 3") (`rpc.activities.write`).
- **Monetization:** `getSkus`, `getEntitlements`, `startPurchase` (Discord-handled payments).
- **Platform/UI:** `getPlatformBehaviors`, layout mode events (focused / picture-in-picture / grid), `setOrientationLockState` and `ORIENTATION_UPDATE` (mobile), `THERMAL_STATE_UPDATE` (mobile), `setConfig` (whether PiP is interactive), `encourageHardwareAcceleration`.
- **Other:** `openExternalLink`, `initiateImageUpload`, `userSettingsGetLocale`, `getRelationships` (needs Social SDK access).

**How it launches:** from the App Launcher via the app's single global **Entry Point command** (created automatically as "Launch"), or by a bot responding to a slash command/button with the `LAUNCH_ACTIVITY` (type 12) interaction callback. Launches can happen in guild voice channels, guild text channels, DMs and group DMs (an instance's location `kind` is `gc` = guild channel or `pc` = private channel, i.e. DM/GDM).

### 2. "Maximum Participants" and instances

**Maximum Participants is launcher metadata.** Discord's [Assets and Metadata guide](https://docs.discord.com/developers/activities/development-guides/assets-and-metadata) says: "The max participants indicate the maximum number of players for your application." It's shown on the activity's card in the App Launcher as "Up to X participants"; leaving it blank shows "Unlimited participants". No Discord doc says it is enforced (the instance and SDK docs mention no cap), so treat it as a **label telling users how many people the activity is designed for**, not a hard limit. If an app needs a real cap, it must enforce it itself (e.g. extra participants get a spectator view). (The guide places the field under Settings → General Information; the portal now also shows it under Activities. Either way it's the same per-app setting.)

Your guess about scope is right: it describes **one instance**, not all of Discord.

**What an instance is:** when someone launches the activity in a channel (voice channel, text channel or DM), Discord creates an **activity instance** for that location. Others in that channel see it running and can click "Join Activity", and they all get the **same `instanceId`**. When everyone has left, the instance ends, and the next launch in that channel gets a new `instanceId`. Each user still runs **their own copy of the web page** in their own iframe; the instance is just a shared ID plus a list of connected users.

**What is shared:** Discord shares **nothing about the app's state**. It provides only:

- the shared `instanceId` (available immediately from the SDK constructor; it is the `instance_id` launch query parameter),
- the list of connected participants, and change events,
- the shared channel/guild context (`channel_id`, `guild_id`, `location_id`).

Anything else that should be shared (a game board, a shared canvas, whose turn it is) is the app's job: the server keys the state by `instanceId` and pushes updates to each participant's page, typically over a **WebSocket** (supported through the proxy; **WebRTC is not**). The [Multiplayer Experience guide](https://docs.discord.com/developers/activities/development-guides/multiplayer-experience) says to use the `instanceId` "as a key to save and load shared data".

**Verifying that a client is really in an instance:** the client-supplied `instanceId` is just a query parameter, so the server can check it with `GET /applications/{app_id}/activity-instances/{instance_id}` (**requires a Bot token**). It returns `{application_id, instance_id, launch_id, location: {id, kind, channel_id, guild_id}, users: [user ids]}`, or 404. That lets the server confirm "this user is currently in this instance" before giving them access to its shared state.

### 3. How an activity differs from a plain website

| Aspect | Plain website | Discord activity |
|---|---|---|
| Hosting/origin | Your domain | Served through Discord's proxy at `{clientId}.discordsays.com`; portal URL mappings route paths to your servers |
| Network | Anything | Strict CSP: only requests to your own proxy domain. External APIs must be added as URL mappings (or rewritten with `patchUrlMappings`, which monkey-patches `fetch`/`WebSocket`/XHR) |
| Realtime | WebSocket, WebRTC, WebTransport | WebSocket only; no WebRTC, no WebTransport (yet) |
| Cookies | Normal | Third-party iframe: need `SameSite=None; Partitioned` on the exact `{clientId}.discordsays.com` domain and are unreliable, hence this framework's bearer token |
| Identity | You build login | Provided by Discord via `authorize`; the user is already signed in |
| Links/navigation | Free | Sandboxed. External links must go through `openExternalLink` (Discord shows a confirmation prompt); no address bar, back button or bookmarks |
| Entry | Any URL | Always launches at the mapped root with Discord's query parameters (`frame_id`, `instance_id`, `platform`, `guild_id`, `channel_id`, `location_id`, `custom_id`, `referrer_id`, …). Deep links exist only as `shareLink` + `custom_id` |
| Lifetime | Tab stays open | Ends when the user leaves the activity; nothing persists between launches unless the app stores it server-side |
| UI | Any size | Must work in focused, picture-in-picture and grid layouts, on mobile in portrait/landscape, and within mobile safe areas |
| Content rewriting | None | The proxy rewrites URLs inside served files (why the framework sets `no_sri = true`) |
| Trust | — | Discord says not to trust data from the Discord client; the server verifies (code exchange, `users/@me`, the activity-instance endpoint). Proxied requests can also carry Discord signature headers (`X-Signature-Ed25519`, `X-Signature-Timestamp`) that the server can verify |
| Social context | None | Participants, voice/speaking, channel, guild, invites, Rich Presence, Discord payments |

**Do activities have to be single-page apps?** In practice, yes. [How Activities Work](https://docs.discord.com/developers/activities/how-activities-work) says the SDK is designed for a single-page app, and recommends nesting any other framework inside that top-level SPA and passing messages between them. The reasons:

- The SDK constructor reads `frame_id`, `instance_id` and `platform` from the page's query string and **throws** if any is missing (confirmed in the vendored `discord.js`). Navigating the iframe to another page loses them unless every link copies them.
- Every full page load restarts the SDK handshake and loses in-memory state, including this framework's bearer token, so the user would have to sign in again on every page.

A multi-page site could technically work if every link carried the query string and the token were re-obtained per page, but it fights the platform. Client-side routing inside the SPA (e.g. `leptos_router`) is fine, because it never reloads the iframe.

### What this means for this framework

- The current design (SPA, in-memory bearer token, server-side code exchange, `no_sri`, vendored SDK) already matches the platform's constraints.
- The framework treats the activity purely as a login. Supporting **multiplayer** apps would need, at a minimum:
  - exposing `instanceId` (and maybe `channel_id`/`guild_id`) from `webapp-client` to apps;
  - a server-side way to verify instance membership (the activity-instance endpoint, which needs a **Bot token**, a new config value), and possibly binding a Discord session to its instance;
  - a realtime channel. axum has built-in WebSocket support (its `ws` feature), and with a single Fly machine, instance state could live in memory. Upstash REST isn't suited to pushing updates.
- SDK commands that need no scopes (participants, invite dialog, `shareLink`, `openExternalLink`, layout events) could be exposed via the existing `authenticate_discord_sdk()` path without changing OAuth scopes. Voice events and Rich Presence need extra scopes (`rpc.voice.read`, `rpc.activities.write`) in the `authorize` call.
- **Maximum Participants** needs no framework support: set it to what the app is designed for, and enforce a real limit in the app if needed.

### Considered / not resolved

- Whether Discord enforces Maximum Participants at all (e.g. disabling "Join" when full) is undocumented. It can be checked empirically by setting it to 1 and having a tester try to join.
- Proxy signature validation (`X-Signature-*`) and per-instance session binding are possible hardening steps; not researched in depth here.

## Follow-Up

**Q:** If `instanceId` is ignored (as it is now), does it matter whether people join the same instance or a different one? Couldn't they all interact anyway if the server allows it, e.g. a massively multiplayer game where Discord and web users all play together because the server provides all the interaction?

**A:** Yes. An instance is only a grouping label Discord hands out. Discord doesn't connect participants to each other or isolate them from other instances; every page, in any instance or in a plain browser, is just a client of the same server. With `instanceId` ignored, the server decides who interacts, so one shared world for Discord and web users works. The framework already supports this: both modes end up as the same `Account` calling the same `/api/*`, and a WebSocket from an activity reaches the same server through the proxy. Instances still matter in three ways:

- **What users expect:** people who click "Join Activity" in a channel expect to end up together, usually while talking in voice. In a global world they should land near each other (same zone/party/match). `instanceId` is a ready-made "these people came together" key for that, even if the world itself is shared.
- **Maximum Participants** describes one instance. For a global game it is only a label, so leaving it blank ("Unlimited") is accurate.
- **Per-instance features** (participant list, speaking indicators, invite dialog) only describe the people in that Discord instance, not everyone in the world.

**Q:** The per-instance SDK features don't do anything unless the app writes code to listen for the events or call the commands, correct?

**A:** Correct. They're opt-in: nothing happens inside the app's page unless it calls the command or subscribes to the event (some also need extra scopes in `authorize`). Today the framework only calls `ready`, `authorize` and `authenticate`. What does happen regardless is Discord's own UI around the iframe: it shows who is in the activity in the channel, offers "Join Activity" to others in the channel, keeps voice chat running alongside, and shows the app on users' profiles while they use it. None of that needs app code, and none of it reaches the app unless the app asks.

### Sources

- [Activities overview](https://docs.discord.com/developers/activities/overview)
- [How Activities Work](https://docs.discord.com/developers/activities/how-activities-work)
- [Multiplayer Experience](https://docs.discord.com/developers/activities/development-guides/multiplayer-experience)
- [Networking](https://docs.discord.com/developers/activities/development-guides/networking)
- [User Actions](https://docs.discord.com/developers/activities/development-guides/user-actions)
- [Assets and Metadata](https://docs.discord.com/developers/activities/development-guides/assets-and-metadata)
- [Embedded App SDK reference](https://docs.discord.com/developers/developer-tools/embedded-app-sdk)
- [Application resource: Activity Instance](https://docs.discord.com/developers/resources/application)
- Vendored SDK: `crates/webapp-client/js/discord.js` (constructor parsing of launch query parameters)
