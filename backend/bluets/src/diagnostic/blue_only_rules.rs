// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Enumerated owner and checker subset restrictions without a TS counterpart.

pub(super) const RULES: &[(&str, &str, &str)] = &[
    ("BTS2002", "value import `{}` resolves to declaration module; declaration modules are type-only", "BlueTSC refuses runtime imports of owner declaration modules without an authorized runtime counterpart."),
    ("BTS2002", "declaration module contains a runtime declaration", "BlueTSC's declaration subset contains only the registered type-only declaration forms."),
    ("BTS3000", "duplicate ambient declaration of `{}`", "BlueTSC's owner ambient declaration table does not perform TypeScript declaration merging."),
    ("BTS3001", "function {} is not declared by this page profile", "The page owner has not declared this function in the allowed host profile."),
    ("BTS3001", "object {} is not declared by this page profile", "The page owner has not declared this object in the allowed host profile."),
    ("BTS3003", "cannot prove a computed property write avoids readonly members", "BlueTSC requires a bounded proof that a computed write avoids readonly members."),
    ("BTS3003", "cannot prove a write through an unmodeled receiver avoids readonly members", "BlueTSC refuses a readonly write through a receiver outside its modeled subset."),
    ("BTS3003", "apply arguments must be a fixed-length tuple", "BlueTSC's apply subset requires a fixed tuple even where TypeScript accepts an array."),
    ("BTS1000", "expected a type alias name", "BlueTSC requires a type declaration name; TypeScript recovers this token sequence as expression statements."),
    ("BTS1000", "`declare` must introduce a supported declaration", "This declaration is outside BlueTSC's supported ambient declaration subset."),
    ("BTS1000", "`async` must precede a function declaration in the initial matrix", "BlueTSC's async declaration subset requires a named function declaration."),
    ("BTS1000", "constructor body did not match its source boundary", "BlueTSC's structured constructor parser could not reconcile its recorded source boundary."),
    ("BTS1000", "static block did not match its source boundary", "BlueTSC's structured static block parser could not reconcile its recorded source boundary."),
    ("BTS1000", "accessor body did not match its source boundary", "BlueTSC's structured accessor parser could not reconcile its recorded source boundary."),
    ("BTS1000", "method body did not match its source boundary", "BlueTSC's structured method parser could not reconcile its recorded source boundary."),
];
