// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Original static decorator surfaces; these grant no runtime capabilities.
type DecoratorMetadataObject = Record<string | number | symbol, unknown> & object;
type DecoratorMetadata = DecoratorMetadataObject | undefined;

interface ClassDecoratorContext<Class = any> {
    readonly kind: "class";
    readonly name: string | undefined;
    readonly metadata: DecoratorMetadata;
    addInitializer(initializer: (this: Class) => void): void;
}

interface ClassMethodDecoratorContext<This = unknown, Value extends (this: This, ...args: any[]) => any = (this: This, ...args: any[]) => any> {
    readonly kind: "method";
    readonly name: string | symbol;
    readonly static: boolean;
    readonly private: boolean;
    readonly metadata: DecoratorMetadata;
    readonly access: { has: (object: This) => boolean; get: (object: This) => Value };
    addInitializer(initializer: (this: This) => void): void;
}

interface ClassGetterDecoratorContext<This = unknown, Value = unknown> {
    readonly kind: "getter";
    readonly name: string | symbol;
    readonly static: boolean;
    readonly private: boolean;
    readonly metadata: DecoratorMetadata;
    readonly access: { has: (object: This) => boolean; get: (object: This) => Value };
    addInitializer(initializer: (this: This) => void): void;
}

interface ClassSetterDecoratorContext<This = unknown, Value = unknown> {
    readonly kind: "setter";
    readonly name: string | symbol;
    readonly static: boolean;
    readonly private: boolean;
    readonly metadata: DecoratorMetadata;
    readonly access: { has: (object: This) => boolean; set: (object: This, value: Value) => void };
    addInitializer(initializer: (this: This) => void): void;
}

interface ClassFieldDecoratorContext<This = unknown, Value = unknown> {
    readonly kind: "field";
    readonly name: string | symbol;
    readonly static: boolean;
    readonly private: boolean;
    readonly metadata: DecoratorMetadata;
    readonly access: { has: (object: This) => boolean; get: (object: This) => Value; set: (object: This, value: Value) => void };
    addInitializer(initializer: (this: This) => void): void;
}

interface ClassAccessorDecoratorContext<This = unknown, Value = unknown> {
    readonly kind: "accessor";
    readonly name: string | symbol;
    readonly static: boolean;
    readonly private: boolean;
    readonly metadata: DecoratorMetadata;
    readonly access: { has: (object: This) => boolean; get: (object: This) => Value; set: (object: This, value: Value) => void };
    addInitializer(initializer: (this: This) => void): void;
}

interface ClassAccessorDecoratorTarget<This, Value> {
    get: (this: This) => Value;
    set: (this: This, value: Value) => void;
}

interface ClassAccessorDecoratorResult<This, Value> {
    get?: (this: This) => Value;
    set?: (this: This, value: Value) => void;
    init?: (this: This, value: Value) => Value;
}

type ClassMemberDecoratorContext = ClassMethodDecoratorContext | ClassGetterDecoratorContext | ClassSetterDecoratorContext | ClassFieldDecoratorContext | ClassAccessorDecoratorContext;
type DecoratorContext = ClassDecoratorContext | ClassMemberDecoratorContext;

interface TypedPropertyDescriptor<T> {
    enumerable?: boolean;
    configurable?: boolean;
    writable?: boolean;
    value?: T;
    get?: () => T;
    set?: (value: T) => void;
}
