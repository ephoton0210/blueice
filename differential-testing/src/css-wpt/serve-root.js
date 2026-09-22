// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// A static file server rooted at the whole WPT checkout
// (`development/browser_core/reference/wpt/`), not a single fixture's
// HTML like `../serve-fixture.js`. WPT reftests reference sibling
// files by relative path and shared resources by root-absolute path
// (`/css/support/...`, `/fonts/ahem.css`), matching how the upstream
// `wpt.py` test runner itself serves the whole repository from one
// virtual root -- so this harness needs the same shape, not a
// per-fixture throwaway page.

import { createServer } from "node:http";
import { createReadStream, existsSync, statSync } from "node:fs";
import { extname, join, normalize, sep } from "node:path";

const CONTENT_TYPES = {
  ".html": "text/html; charset=utf-8",
  ".htm": "text/html; charset=utf-8",
  ".xht": "application/xhtml+xml; charset=utf-8",
  ".xhtml": "application/xhtml+xml; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".png": "image/png",
  ".jpg": "image/jpeg",
  ".jpeg": "image/jpeg",
  ".svg": "image/svg+xml",
  ".ttf": "font/ttf",
  ".otf": "font/otf",
  ".woff": "font/woff",
  ".woff2": "font/woff2",
};

/**
 * Serves `root` (a directory) over HTTP on a random local port until
 * `close()` is called. Returns `{ url, close }`, where `url` is the
 * server's own origin (no trailing slash) -- callers build request
 * paths onto it themselves, since a reftest's own root-absolute links
 * need to resolve against that origin, not a fixed `/`.
 */
export function serveRoot(root) {
  return new Promise((resolve, reject) => {
    const server = createServer((req, res) => {
      const requestPath = decodeURIComponent(req.url.split("?")[0]);
      // Reject any attempt to escape `root` via `..` segments before
      // touching the filesystem -- this server only ever needs to
      // expose files already checked out under `root`.
      const normalized = normalize(requestPath).replace(/^(\.\.[/\\])+/, "");
      const filePath = join(root, normalized);
      if (!filePath.startsWith(root + sep) && filePath !== root) {
        res.writeHead(403);
        res.end();
        return;
      }
      if (!existsSync(filePath) || !statSync(filePath).isFile()) {
        res.writeHead(404);
        res.end();
        return;
      }
      const contentType = CONTENT_TYPES[extname(filePath).toLowerCase()] ?? "application/octet-stream";
      res.writeHead(200, { "Content-Type": contentType });
      createReadStream(filePath).pipe(res);
    });
    server.on("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const { port } = server.address();
      resolve({
        url: `http://127.0.0.1:${port}`,
        close: () => new Promise((res) => server.close(() => res())),
      });
    });
  });
}
