# tableski.io landing

Single self-contained `index.html` (no external assets, light/dark aware).

Deploy anywhere that serves static files over HTTPS, e.g.:
- **nginx/apache on the web host:** copy `index.html` to the site root.
- **Cloudflare (if DNS is there):** proxied A record + origin on plain HTTP is fine —
  edge terminates TLS; or Cloudflare Pages serving this directory.

Health check: `curl -sI https://tableski.io | head -1` → `HTTP/2 200`.
