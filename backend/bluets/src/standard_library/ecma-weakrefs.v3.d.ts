// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Original declarations for the ECMAScript 2021 weak-reference surface.
interface WeakKeyTypes {
    object: object;
}
type WeakKey = WeakKeyTypes[keyof WeakKeyTypes];

interface WeakRef<T extends WeakKey> {
    deref(): T | undefined;
}

interface WeakRefConstructor {
    new<T extends WeakKey>(target: T): WeakRef<T>;
    readonly prototype: WeakRef<WeakKey>;
}
declare const WeakRef: WeakRefConstructor;

interface FinalizationRegistry<T> {
    register(target: WeakKey, heldValue: T, unregisterToken?: WeakKey): void;
    unregister(unregisterToken: WeakKey): boolean;
}

interface FinalizationRegistryConstructor {
    new<T>(cleanupCallback: (heldValue: T) => void): FinalizationRegistry<T>;
    readonly prototype: FinalizationRegistry<any>;
}
declare const FinalizationRegistry: FinalizationRegistryConstructor;
