// Cloudflare Worker: CORS proxy for xkcd API and images.
// Deploy: npx wrangler deploy
// Free tier: 100k requests/day.

const ALLOWED_ORIGINS = [
  "https://samoylenkodmitry.github.io",
  "http://localhost",
  "http://127.0.0.1",
];

function isAllowedOrigin(origin) {
  if (!origin) return false;
  return ALLOWED_ORIGINS.some((allowed) => origin.startsWith(allowed));
}

const ALLOWED_UPSTREAM = ["https://xkcd.com/", "https://imgs.xkcd.com/"];

function isAllowedUpstream(url) {
  return ALLOWED_UPSTREAM.some((prefix) => url.startsWith(prefix));
}

export default {
  async fetch(request) {
    const origin = request.headers.get("Origin") || "";

    if (request.method === "OPTIONS") {
      return new Response(null, {
        headers: corsHeaders(origin),
      });
    }

    const url = new URL(request.url);
    const target = url.searchParams.get("url");

    if (!target) {
      return new Response("Missing ?url= parameter", { status: 400 });
    }

    if (!isAllowedUpstream(target)) {
      return new Response("Upstream not allowed", { status: 403 });
    }

    if (!isAllowedOrigin(origin)) {
      return new Response("Origin not allowed", { status: 403 });
    }

    const upstream = await fetch(target, {
      headers: { "User-Agent": "cranpose/0.1" },
    });

    const response = new Response(upstream.body, {
      status: upstream.status,
      headers: {
        "Content-Type": upstream.headers.get("Content-Type") || "application/octet-stream",
        ...corsHeaders(origin),
      },
    });

    return response;
  },
};

function corsHeaders(origin) {
  return {
    "Access-Control-Allow-Origin": origin,
    "Access-Control-Allow-Methods": "GET, OPTIONS",
    "Access-Control-Allow-Headers": "Content-Type",
    "Access-Control-Max-Age": "86400",
  };
}
