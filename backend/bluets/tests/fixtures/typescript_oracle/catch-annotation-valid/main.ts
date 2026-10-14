// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
export function a(): string { try { return "a"; } catch (err: unknown) { return "b"; } }
export function b(): string { try { return "a"; } catch (err: any) { return err; } }
export function c(): string { try { return "a"; } catch { return "b"; } }
export function d(): string { try { return "a"; } catch (err) { return "b"; } }
export function e(): string { try { return "a"; } finally { } }
export function f(): string { try { } finally { return "x"; } }
export function g(): string { try { throw 1; } catch (err: unknown) { throw err; } }
