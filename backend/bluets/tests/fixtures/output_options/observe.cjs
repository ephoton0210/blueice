// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const fs = require('fs'), path = require('path');
async function main() {
 const [directory, kind] = process.argv.slice(2);
 const file = path.join(directory, 'main.js');
 const value = kind === 'CommonJS' ? require(file) : await import('data:text/javascript;base64,' + fs.readFileSync(file).toString('base64'));
 process.stdout.write(JSON.stringify({result: value.result, text: value.text}) + '\n');
}
main().catch(error => {console.error(error); process.exitCode = 1;});
