// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// A throwaway local HTTP server for one fixture's HTML -- both
// BlueIce (`blueice-net` only fetches `http://`/`https://`, per its
// own module docs; no `file://`/`data:` support) and a real Chromium
// tab need an actual URL to navigate to, and using the same URL
// scheme for both keeps the comparison honest (no "one side got a
// data: URI, the other got a file" asymmetry).

import { createServer } from "node:http";

/**
 * Serves `html` at `/` on a random local port until `close()` is
 * called. Returns `{ url, close }`.
 */
export function serveHtml(html) {
  return new Promise((resolve, reject) => {
    const server = createServer((_req, res) => {
      res.writeHead(200, { "Content-Type": "text/html; charset=utf-8" });
      res.end(html);
    });
    server.on("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const { port } = server.address();
      resolve({
        url: `http://127.0.0.1:${port}/`,
        close: () => new Promise((res) => server.close(() => res())),
      });
    });
  });
}
