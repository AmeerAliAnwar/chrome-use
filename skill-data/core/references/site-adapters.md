# Site adapters — the cheapest path for "read structured data from site X"

Before you `open` + `snapshot` + click your way through GitHub/Reddit/Bilibili/etc.,
check whether a **site adapter** already exists. An adapter is a community-written JS
function that hits the site's own JSON API *from inside your logged-in tab* and returns
clean structured data — no clicking, no scraping, no screenshots. It's the same idea as
`eval`, packaged per-site.

```bash
chrome-use site update                       # one-time: fetch community + official packs
chrome-use site list                         # what's installed (github/issues, reddit/search, …)
chrome-use site info github/issues           # an adapter's args + which domain it runs on
chrome-use site github/issues owner/repo --json   # run it → JSON (navigates there for you)
```

- Positional args fill the adapter's declared args **in order**; `--key value` overrides by name.
  If a `--key` name collides with a reserved global flag (`--state`, `--profile`, `--session`, `--timeout`, …)
  it is consumed globally and never reaches the adapter — the CLI warns, and you should pass it
  **positionally** or after `--`: `chrome-use site demo/pr-list -- --state closed`.
- It navigates to the adapter's domain (reusing the current tab if you're already on it), so
  login-gated feeds (`bilibili/feed`, `twitter/...`) work because they run as *you*.
- If no adapter fits, fall back to the normal `snapshot`/`eval` loop. chrome-use fetches and
  runs two default sources: the [bb-sites](https://github.com/epiral/bb-sites) community pack
  and the official [chrome-use-sites](https://github.com/leeguooooo/chrome-use-sites) pack.

> **Auto-trigger — act on it.** chrome-use keeps both packs synced automatically (first use +
> weekly), and when you `open`/`navigate`/`snapshot` a page whose domain has adapters it tells
> you: a `site adapters for <domain>` line on stderr, and a `siteAdapters: {domain, commands}`
> field in `--json`. **When you see that, prefer the listed `site <name>/<cmd>` over snapshot+click
> for reading data** — it's the cheaper, more reliable path and it's already installed. You don't
> need to run `site update` yourself; just use the command it names. (Only on a brand-new setup
> where the packs haven't been fetched yet, a named `site <name>/<cmd>` may say it's not installed —
> run `site update` once, then re-run the command.)
