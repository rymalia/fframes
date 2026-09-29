---
session_id: 33037f53-7301-489c-b5f2-47293ff1ee55
date: 2026-09-29
time: "10:17 AM PDT – 10:32 AM PDT"
project: fframes
---

# Session Summary: Comparing and cleaning up the installed video skills

### Overview

Compared the `/video-shotcraft` and `/fframes-video` agent skills. Then updated the installed `fframes-video` skill from this repo's copy, and removed the promotional content from the installed `video-shotcraft` skill. No files in the fframes repository were changed; all edits were to installed skills under `~/.agents/skills/`.

### Key Decisions Made

| Decision | Rationale |
|----------|-----------|
| **Updated fframes-video with `rsync -a --delete` from `skills/fframes-video/` instead of `npx skills update`** | The local repo copy was known to have the fixes (zsh-safe `R()` function, macOS 27 troubleshooting). A direct sync was simple, and `diff -r` could confirm it. |
| **Backed up video-shotcraft before editing** | The skill isn't a git checkout (installed by `npx skills`), so there was no other way to undo the edits. |
| **Kept the online gallery links and `references/shots/ATTRIBUTION.md`** | The gallery is a working tool for choosing shots; ATTRIBUTION credits the sources of the techniques. Neither is promotion. |
| **Kept the video-talkcraft mentions in the workbench docs** | They record where the code came from and why the two workbenches use different ports (5198/5199), which matters for running them. |
| **Kept an English `README.md` with the promo removed; deleted `README_CN.md`/`README_JA.md`** | README.md is the only English overview in a skill that is mostly Chinese. |
| **Kept the Remotion license note, but only when the user asks about commercial use** | It is real licensing information. It used to be part of the promotional closing steps. |
| **Recommended not to machine-translate the Chinese content** | Claude reads Chinese fine. Translation could mangle tuned terms, cross-references and pitfall comments in `.tsx` files, and would block clean upstream updates. The alternative offered: Claude translates selected files into a separate `en/` folder. |

### Changes Made

| Change | Detail |
|--------|--------|
| **fframes-video synced** | `~/.agents/skills/fframes-video/` now matches `skills/fframes-video/` exactly (`diff -r` clean). It gains the `R() { cargo run --release -- "$@"; }` shell function and the macOS 27 troubleshooting entries (Skia `_Traits` bindgen error, proc-macro "LINKEDIT" error). |
| **video-shotcraft `SKILL.md`** | Replaced the three closing steps (credit the author with X/Douyin/Xiaohongshu links; submit to the showcase; CapCut export) with just the CapCut (Jianying) export offer, plus a Remotion license note given only on request. |
| **video-shotcraft `references/pipeline.md`** | Trimmed its copy of those closing steps the same way. |
| **Deleted files** | `.github/` (showcase issue template, showcase-publish script and workflow, deploy-pages, pr-checks), `README_CN.md`, `README_JA.md`, `gallery/showcase.html`, `gallery/showcase.js`, `gallery/build-seo.py`, `gallery/sitemap.xml`, `gallery/robots.txt`, `gallery/og-image.jpg`, `gallery/llms.txt`. |
| **Gallery pages** | `library.html`: removed the follow-me menu, the "Star on GitHub" badge and the showcase badge. `index.html`: removed the showcase nav link, the GitHub star CTA button and the script that fetched the star count from the GitHub API. `fetch-media.sh`: removed the showcase-media download block. `.gitignore`: removed the showcase entries. |
| **`README.md`** | Removed the showcase, star, AtomGit and Trendshift badges, the language switcher, the showcase links, the "What's new" section (which advertised the sister project), "Follow me" and "Star history". Renamed "Showcase" to "Demo film". |
| **Backup** | Original saved at `<session scratchpad>/video-shotcraft.orig` (a session temp directory, so it may not last). |

### Research Performed

- Read both `SKILL.md` files in full, and diffed the installed fframes-video against the repo copy (2 hunks: the zsh `R` fix and the macOS 27 troubleshooting).
- Found the install source for both skills in `~/.agents/.skill-lock.json`: `dmtrKovalenko/fframes` and `Vincentwei1021/video-shotcraft`. Both are linked into `~/.claude/skills/`.
- Grepped video-shotcraft for promo markers (social handles, showcase, stargazers, Trendshift, star-history, talkcraft, AtomGit) before editing, and again after. The final grep for social links, showcase pages, stargazers, Trendshift and star-history found no matches.
- Measured the Chinese content with a Python scan: 334 of 335 `.md` files contain Chinese (~292k CJK characters), 467 of 485 `.tsx` files (~116k), plus smaller amounts in `.json`, `.ts`, `.py` and others. The skill is 64 MB installed.

### Summary Statistics

- Skills updated: 2
- video-shotcraft files/dirs deleted: 10 (including the `.github/` tree)
- video-shotcraft files edited: 7 (`SKILL.md`, `references/pipeline.md`, `gallery/library.html`, `gallery/index.html`, `gallery/fetch-media.sh`, `.gitignore`, `README.md`)
- fframes repository files changed: 0 (only this summary)

### Discoveries / Handoff Notes

- **Running `npx skills update` would undo the cleanup** by reinstalling video-shotcraft from upstream. Redo the edits (or restore from a saved copy) after any update.
- video-shotcraft is built on Remotion (React/TSX), not fframes. Its shot recipe cards include tuned timing and easing tables that could be ported to fframes `timeline!` code.
- In video-shotcraft, `gallery/styles.css` and `gallery/translations.js` still contain unused showcase and follow styles and strings. They were left in place because they have no visible effect.

### Unfinished Work

- Optional: have Claude translate selected video-shotcraft files (for example `SKILL.md`, `references/pipeline.md`, `references/aesthetic-rules.md`) into a separate `en/` folder, if the user wants to read them.
- Optional: copy the backup out of the session scratchpad if it should be kept.
