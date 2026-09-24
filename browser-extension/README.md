# OpenFlow for Google Meet

A small Chrome extension that tells the OpenFlow app on this Mac who is speaking in a Google Meet call, so the
meeting transcript shows names ("Priya: …") instead of "Them". OpenFlow records and transcribes without it; the
extension only adds names.

## Install (unpacked)

1. Open `chrome://extensions` and turn on **Developer mode**.
2. Click **Load unpacked** and choose this `browser-extension` folder.
3. Its ID must be `mbddbmiendbipklcehlgelanomjpoppf`, which is fixed by the `key` in `manifest.json`. OpenFlow
   accepts requests from that ID only.

Edge, Brave, Arc and other Chromium browsers load it the same way.

## What it does

- Runs only on `https://meet.google.com/*`.
- **Speaker:** every 400 ms it finds the participant tiles (`data-participant-id`) and works out whose tile is
  showing speaking activity. Meet animates a talking person's audio indicator, so their tile gets a burst of DOM
  changes; a silent tile stays still. It reports the busiest non-self tile's name.
- **Captions:** if you turn on Meet's captions (the CC button), each caption block ("Name", then what they said) is
  also sent. OpenFlow matches caption words against its own transcript lines, which is more reliable than tile
  activity when two people talk in quick turns.

## Privacy

- Only participant names and caption text leave the page.
- They go only to `http://127.0.0.1:8770/meet`, which is OpenFlow on this Mac, and nowhere else.
- No audio, no meeting URL or code, and no cookies are sent.
- OpenFlow uses the names only while a meeting is being recorded. It ignores them otherwise.

## Testing

`node --test browser-extension/content-script.test.js` runs the pure parsing logic. The DOM side has to be checked
in a real call: join a Meet with a second device, record in OpenFlow, and speak from each side.
