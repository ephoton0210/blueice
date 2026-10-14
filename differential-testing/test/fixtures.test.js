// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import assert from "node:assert/strict";
import test from "node:test";
import { htmlForFixture } from "../src/fixtures.js";

test("embeds a fixture's author stylesheet inside its explicit head", () => {
  const fixture = { sections: { data: "<html><head><title>x</title></head><body>body</body></html>", css: "p { color: red; }" } };
  assert.equal(
    htmlForFixture(fixture),
    "<html><head><title>x</title><style>p { color: red; }</style></head><body>body</body></html>",
  );
});

test("uses an implicit head when a fixture has CSS but no explicit head", () => {
  const fixture = { sections: { data: "<p>body</p>", css: "p { color: red; }" } };
  assert.equal(htmlForFixture(fixture), "<style>p { color: red; }</style><p>body</p>");
});
