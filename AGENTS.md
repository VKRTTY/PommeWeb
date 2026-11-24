# Instructions for coding agents

This repo is the browser port of Pomme. Read [HANDOFF.md](HANDOFF.md) before
doing anything: it says where the code lives (Rust is edited in `~/rust mc`,
web code here), how to build and test, and what's left.

## "add play"

When the owner says **"add play"** (or "continue", "keep going"), it means:
continue the port toward fully playable in the browser.

1. Read HANDOFF.md. Work through its "Not done, in suggested order" list
   from the top, starting with fast chunk loading.
2. After each piece works, check it in Chrome with `scripts/browser-check.mjs`
   and take gameplay screenshots.
3. Run `./sync.sh`, update `docs/progress.json` and redraw the map with
   `python3 docs/progress.py`, then add screenshots to the README.
4. Commit and push, authored as `VKRTTY <360361036+VKRTTY@users.noreply.github.com>`,
   with no AI co-author lines.
5. Keep HANDOFF.md's "State" section current, so the next session can
   continue.

Never break the desktop build (`cargo check -p pomme-client` in `~/rust mc`),
never push Pomme's full source here, and keep the loading screen vanilla-red
with all text in the Minecraft font.
