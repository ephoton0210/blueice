// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Original versioned declarations for the owned ECMAScript surface.
interface AggregateError { name: string; message: string; errors: any[]; stack?: string; }
type AggregateErrorConstructor = (errors: any[], message?: string) => AggregateError;
declare const AggregateError: AggregateErrorConstructor;
