// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Preserve every decoded segment, name and metadata field, including trivia gaps.
const assert = require('node:assert/strict');
const alphabet = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';
function fields(encoded) {
  assert(encoded.length > 0, 'empty map segment');
  const values = []; let value = 0, shift = 0;
  for (const character of encoded) {
    const digit = alphabet.indexOf(character);
    assert(digit >= 0 && shift <= 45, 'invalid or oversized VLQ');
    value += (digit & 31) * 2 ** shift;
    assert(Number.isSafeInteger(value), 'VLQ overflow');
    if (digit & 32) shift += 5;
    else { values.push(value & 1 ? -Math.floor(value / 2) : value / 2); value = shift = 0; }
  }
  assert.equal(shift, 0, 'unfinished VLQ'); assert([1, 4, 5].includes(values.length), 'segment arity');
  return values;
}
function decode(text, generated, options = {}) {
  const map = JSON.parse(text);
  assert.equal(map.version, 3); assert(Array.isArray(map.sources) && map.sources.every(v => typeof v === 'string'));
  assert(Array.isArray(map.names) && map.names.every(v => typeof v === 'string'));
  assert.equal(typeof map.mappings, 'string');
  const widths = generated === undefined ? null : generated.split('\n').map(line => line.length);
  let contents;
  if (Object.hasOwn(map, 'sourcesContent')) {
    assert(Array.isArray(map.sourcesContent) && map.sourcesContent.length === map.sources.length);
    contents = map.sourcesContent.map(source => {
      assert(source === null || typeof source === 'string');
      return source === null ? null : source.split('\n').map(line => line.length);
    });
  }
  let source = 0, line = 0, column = 0, name = 0;
  const segments = [], transitions = [], coordinateIssues = [];
  const coordinate = (valid, kind, segment) => {
    if (valid) return;
    // Native-only recording retains evidence of upstream bad coordinates.
    // Actual compiler-map validation uses the strict default.
    assert(options.recordInvalidCoordinates, kind);
    coordinateIssues.push({ kind, segment: [...segment] });
  };
  for (const [generatedLine, encodedLine] of map.mappings.split(';').entries()) {
    if (!encodedLine) continue;
    let generatedColumn = 0, previous = -1, mapped = false;
    for (const encoded of encodedLine.split(',')) {
      const values = fields(encoded); assert(values[0] >= 0, 'negative generated delta');
      // Pinned Native class closures can emit distinct source positions at
      // one generated coordinate. Preserve both, in order; lookup is last-wins.
      generatedColumn += values[0]; assert(generatedColumn >= previous, 'unordered generated columns'); previous = generatedColumn;
      const segment = [generatedLine, generatedColumn];
      if (widths) coordinate(generatedLine < widths.length && generatedColumn <= widths[generatedLine], 'generated coordinate bounds', segment);
      if (values.length > 1) {
        source += values[1]; line += values[2]; column += values[3];
        assert(source >= 0 && source < map.sources.length && line >= 0 && column >= 0, 'original coordinate bounds');
        segment.push(source, line, column);
        if (contents?.[source]) coordinate(line < contents[source].length && column <= contents[source][line], 'UTF-16 source bounds', segment);
        if (values.length === 5) { name += values[4]; assert(name >= 0 && name < map.names.length, 'name index bounds'); segment.push(name); }
      }
      const nextMapped = values.length > 1;
      if (mapped !== nextMapped) { transitions.push([generatedLine, generatedColumn, nextMapped]); mapped = nextMapped; }
      segments.push(segment);
    }
  }
  const { mappings, ...metadata } = map;
  return { metadata, segments, transitions, coordinateIssues };
}

function artifacts(outputs, options = {}) {
  const maps = {};
  for (const [name, text] of Object.entries(outputs)) {
    if (name.endsWith('.map')) {
      const emitted = name.slice(0, -4); assert(Object.hasOwn(outputs, emitted), name + ': map without artifact');
      maps[name] = { output: emitted, kind: /\.d\.(?:ts|mts|cts)$/.test(emitted) ? 'declaration' : 'javascript',
        text, decoded: decode(text, outputs[emitted], options) };
    } else if (/\.(?:js|mjs|cjs)$/.test(name)) {
      const match = /(?:^|\n)\/\/# sourceMappingURL=data:application\/json;base64,([^\r\n]+)\r?\n?$/.exec(text);
      if (match) {
        const map = Buffer.from(match[1], 'base64').toString('utf8');
        maps[name + '#inline'] = { output: name, kind: 'javascript', text: map, decoded: decode(map, text, options) };
      }
    }
  }
  return maps;
}
module.exports = { fields, decode, artifacts };
