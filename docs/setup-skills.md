# Setting Up Claude Code Skills

This repo ships Claude Code skills under `skills/`. They must be symlinked into
`~/.claude/skills/` before Claude Code can discover them.

## cognitive-portrait

### One-time setup

```bash
# 1. If you have an old external copy, remove it first
[ -d ~/.claude/skills/cognitive-portrait ] && \
  [ ! -L ~/.claude/skills/cognitive-portrait ] && \
  rm -rf ~/.claude/skills/cognitive-portrait

# 2. Create the symlink (replace <repo-path> with the absolute path to this repo)
ln -s <repo-path>/skills/cognitive-portrait ~/.claude/skills/cognitive-portrait
```

Example with a typical clone location:

```bash
ln -s ~/Desktop/code/AI/tool/refine/skills/cognitive-portrait \
      ~/.claude/skills/cognitive-portrait
```

### Verify

```bash
ls -la ~/.claude/skills/cognitive-portrait
# Should show: ... -> /path/to/repo/skills/cognitive-portrait
```

If it shows a plain directory instead of a symlink (`->` arrow), you still have
the old external copy. Delete it and re-run step 2 above.

### Pattern

This follows the `~/.claude/skills/SYMLINKS.md` convention already in use.
Skills live in version control; `~/.claude/skills/` holds symlinks only.
