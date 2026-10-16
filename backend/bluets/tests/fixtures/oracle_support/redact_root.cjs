// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

module.exports = function redactRoot(text, root) {
    for (const spelling of new Set([root, root.replaceAll('\\', '/')])) {
        text = text.replaceAll(`${spelling}\\`, '<root>/')
            .replaceAll(`${spelling}/`, '<root>/')
            .replaceAll(spelling, '<root>');
    }
    return text;
};
