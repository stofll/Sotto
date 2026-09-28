# Overlay appearance references

Examples collected while shaping [Overlay customization](overlay-customization.md). Each one is a look, a motion, or a placement habit worth watching. None of them is a feature Sotto has to copy, and none of them changes the behavior described in [Overlay appearance](overlay.md).

The list was gathered on 26 September 2026 from product docs, repositories, and posts on X. Clips are the thing to watch. The words under each link only say what to look for.

## Shell and what sits inside it

- [Halogen case study](https://x.com/sashabirukoff/status/2103156002220589129), @sashabirukoff. One thin stroke rearranges itself: a vertical capsule with two dot matrices, a horizontal pill with a timer, a card with one action, a rounded square with a short list.
- [Halogen launch](https://x.com/sashabirukoff/status/2054668597562396994), same author. The same kind of indicator sitting beside a screen recording, rather than inside a settings form.
- [Chip stretches into a pill](https://x.com/mnowakdesign/status/1974488985943912458), @mnowakdesign. A compact chip grows into a pill that holds a slider, then shrinks back and keeps the new value in its label.
- [Soft pill notifications](https://x.com/mnowakdesign/status/1986056474305630371), same author. A stack of soft pills, useful for how a status line can sit under a recording row without a second chrome.
- [One shape, many states](https://x.com/RaphaelAubryy/status/2103401350897823974), @RaphaelAubryy. One object changes size and corner radius while its contents swap under a short blur. Useful as a transition between recording and processing, at a size that already exists.
- [VoiceInk notch recorder](https://github.com/Beingpax/VoiceInk), Beingpax. A mini recorder panel and a separate notch recorder. Release notes call out a live transcript that exists only on the notch recorder. [Issue 572](https://github.com/Beingpax/VoiceInk/issues/572) shows the panel appearing with idle bars, then switching to a live waveform about a second later.
- [VoiceInk waveform position](https://github.com/BryceWG/VoiceInk), BryceWG. A Windows build whose settings include where the waveform sits.

## Level

- [Voice effect](https://x.com/Jakubantalik/status/2100620796796502264), @Jakubantalik. Three voice-glow drawings, several states, dark and light, no dependencies. The same construction is already ported into [desktop/src/overlay/voice-glow](../desktop/src/overlay/voice-glow) from voice-glow 0.2.0. Clip and library: [libraries.dev/voice](https://libraries.dev/voice). Package notes: [voice-glow on npm](https://www.npmjs.com/package/voice-glow).
- [Border beam, orbs, metal, gooey](https://x.com/Jakubantalik/status/2095551141367173608), same author. The family the glow belongs to. Border beam is the traveling edge; voice glow is the edge that stays put and rises with loudness. Home: [libraries.dev](https://libraries.dev).
- [AI Agent Audio Waves](https://x.com/kvnkld/status/2100225826474295751), @kvnkld. A low waveform strip. Round buttons at the ends stay clear of the signal. Code: [aicss.dev/components/audio-waves](https://www.aicss.dev/components/audio-waves). The same clip is reposted by [@uidesign_studio](https://x.com/uidesign_studio/status/2102805462353596603).
- [Amp dictation waveform](https://x.com/sqs/status/2082398451674005625), @sqs. A short dictation clip whose waveform was replaced with a calmer one so listening is obvious.
- [A small recording indicator](https://x.com/maria_rcks/status/2098992194166087720), @maria_rcks. A very short pill whose only job is to show that recording is on.
- [Dot matrix on music](https://x.com/marcowitss/status/2059435525665386506), @marcowitss. A grid of dots reacting to loudness, with scene and color switches beside it.
- [CSS voice visualizer](https://github.com/jessekorzan/voice-visualizer), Jesse Korzan. Bars and dots animated with transforms. Demos: [voice-visualizer-001](https://voice-visualizer-001.netlify.app/) and [voice-visualizer-002](https://voice-visualizer-002.netlify.app/).

## Live text instead of the level

- [Pill shows the live transcript](https://x.com/LuliYanng/status/2103155717033324966), @LuliYanng, about @Type_Whisper. Halfway through a sentence the pill swaps the waveform for the words, so listening is visible. Cleanup still runs once, after the key is released.
- [Dictation inside the product chrome](https://x.com/shadcncraft/status/2091825664450265376), @shadcncraft. A live waveform in the composer, next to a command palette, rather than a window of its own.
- [Flowtype](https://x.com/yash_goyal_dev/status/2103407786470109662), @yash_goyal_dev. Local Whisper on a Mac. A tiny waveform is the only sign that the hotkey is listening.
- [Aqua edit chip](https://aquavoice.com/guide/edit-mode). A small chip above the pill names what is selected, for example "12 words selected".
- [Aqua realtime](https://aquavoice.com/realtime). The unfinished tail of a phrase is drawn softer and catches up. Text lands in the other app only when the session ends.

## Where it sits

- [Wispr Flow Bar](https://docs.wisprflow.ai/articles/1790396454-move-and-dock-the-flow-bar-on-desktop). A small bar with resting, recording, and processing states. It can be dragged and docked to an edge. Empty areas pass clicks through. New installs hide it. White bars confirm recording. Cancel and confirm live on the bar.
- [Let me move the Wispr pill](https://x.com/alialobai1/status/2067055496293396923), @alialobai1. The floating pill covers something important, and which thing depends on the app.
- [Choose where the pill appears](https://x.com/FreestyleVoice/status/2067506911327719786), @FreestyleVoice. A reply that builds the missing choice into another dictation pill.
- [PillFloat](https://x.com/AKrishnaAkhil/status/2041538993003876712), @AKrishnaAkhil. A separate tool that moves the Wispr pill. The same request again: [@yuvraj_io](https://x.com/yuvraj_io/status/2067139157244461260). A later note says Wispr added a limited move of its own: [that follow-up](https://x.com/AKrishnaAkhil/status/2093447348685066335).
- [Superwhisper recording window](https://superwhisper.com/docs/get-started/interface-rec-window). A full window with waveform, mode, stop, cancel, and realtime text. A mini window can stay on screen. The waveform stays visible, stop appears on hover, and a right-click opens a short menu.
- [SuperIsland](https://x.com/nullbytes00/status/2042392333631832331), @nullbytes00. The Mac notch as one home. Compact at rest, then a player, calendar, or timer when there is something to show.
- [A Windows island](https://x.com/Yappologistic/status/2085285746299281488), @Yappologistic. A small surface on the display edge. Idle shows the time. Approaching the pointer opens media controls and status.
- [Session status in the notch](https://x.com/FarouqAldori/status/1998432542173569438), @FarouqAldori. A compact island shows a coding session and asks for permission without pulling the main window forward.
- [NotchBrowser](https://x.com/kirtandopamine/status/2103165258433278229), @kirtandopamine. A browser that lives in the notch and stays out of the middle of the screen. [NotchBrow](https://x.com/guyonlocalhost/status/2103594516825710723), @guyonlocalhost, is the same idea and adds a notch on displays that do not have one.
- [openNook](https://github.com/prodBirdy/openNook). Compact housing, expand on hover or click, Escape collapses it. Without a camera housing the same block sits at the top center.
- [Dynamic Island for Windows](https://github.com/devcode90/Dynamic-Island-for-Windows). Named corners and edge centers, a separate offset for the collapsed and expanded states, a scale, eight palettes, and three shells: pill, notch, Fluent. Blur and the stroke are separate layers.
- [NotchNook and the Mac notch apps](https://www.howtogeek.com/these-apps-turn-your-macbook-notch-into-a-dynamic-island/). Open on click, hover, or swipe. Transparency and density are settings. Two fixed sizes, compact and expanded.
- [Overwhisper](https://github.com/OverseedAI/overwhisper). The overlay exists for the session: a configurable position, a waveform while recording, a transcribing state after it. Nothing while idle.
- [PixelWhisper](https://x.com/DogeGameDev/status/2027469830610104786), @DogeGameDev. A floating button on Android: tap, speak, insert into the focused field.
- [RunAnywhere dictation overlay](https://x.com/RunAnywhereAI/status/2083334596519919848), @RunAnywhereAI. A floating overlay for on-device speech-to-text on a phone NPU.
- [The macOS microphone that stays](https://apple.stackexchange.com/questions/363545/what-is-the-small-microphone-icon-floating-on-the-screen-in-macos-and-how). Voice Control and Dictation leave a small mic on screen, and it is easy for that mic to get stuck. A reason for an indicator that arrives with a session and leaves with it.

## Motion inside a fixed size

- [Tab pill choreography](https://x.com/msllrs/status/2050483859070808312), @msllrs. The pill slides toward the hovered icon, the label fades in with a little blur, and dividers on the path hide. A model for a slot appearing inside a window whose size does not change.
- [dynisland timing](https://github.com/cr3eperall/dynisland/blob/main/default.scss). Width and height move for 600 ms on `cubic-bezier(0.2, 0.55, 0.25, 1)`. Opacity and shift use their own curves. A sample for animating between two rows that already exist in the size table.
