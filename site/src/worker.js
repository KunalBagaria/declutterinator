// Serves the static site, plus `/install` as a plain-text shell script so that
//   curl -fsSL https://declutter.kunalbagaria.com/install | sh
// works, and so browsers show the script instead of downloading it.

const SECURITY = {
  "content-security-policy":
    "default-src 'none'; script-src 'self'; style-src 'self'; font-src 'self'; img-src 'self'; connect-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'",
  "x-content-type-options": "nosniff",
  "referrer-policy": "strict-origin-when-cross-origin",
  "permissions-policy": "camera=(), microphone=(), geolocation=()",
  "strict-transport-security": "max-age=31536000; includeSubDomains",
};

const SHORT = "public, max-age=300";
const DAY = "public, max-age=86400";
const FOREVER = "public, max-age=31536000, immutable";

function cacheFor(path) {
  if (path.startsWith("/fonts/") || /^\/releases\/v[^/]+\//.test(path)) return FOREVER;
  if (path === "/install" || path === "/install.sh" || path === "/releases/latest.txt") return SHORT;
  if (/\.(png|svg|webmanifest)$/.test(path)) return DAY;
  return null; // let the asset defaults (with ETags) apply
}

// Machine-facing files: fetchable, but never shown as search results.
function noindex(path) {
  return (
    path === "/install" ||
    path === "/install.sh" ||
    path.startsWith("/releases/") ||
    /^\/[0-9a-f]{32}\.txt$/.test(path) // IndexNow key
  );
}

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    let path = url.pathname;

    let response;
    if (path === "/install" || path === "/install/") {
      path = "/install";
      const script = await env.ASSETS.fetch(new URL("/install.sh", url));
      response = new Response(script.body, script);
      response.headers.set("content-type", "text/plain; charset=utf-8");
    } else {
      response = await env.ASSETS.fetch(request);
      response = new Response(response.body, response);
    }

    if (path.endsWith(".sh") || path.endsWith(".sha256") || path.endsWith(".txt")) {
      response.headers.set("content-type", "text/plain; charset=utf-8");
    }
    for (const [name, value] of Object.entries(SECURITY)) response.headers.set(name, value);
    if (noindex(path)) response.headers.set("x-robots-tag", "noindex");
    const cache = cacheFor(path);
    if (cache && response.ok) response.headers.set("cache-control", cache);
    return response;
  },
};
