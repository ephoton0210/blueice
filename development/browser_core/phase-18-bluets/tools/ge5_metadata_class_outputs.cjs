// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Each preserved non-entry module is proved from its own complete original AST.
function compare(ts,native,actual,options,text){const original=ts.createSourceFile('provider.ts',text,ts.ScriptTarget.Latest,true);return original.statements.some(node=>ts.isExportDeclaration(node)&&node.moduleSpecifier)?require('./ge5_metadata_class_barrel.cjs').compare(ts,native,actual,options,text):require('./ge5_metadata_class_provider.cjs').compare(ts,native,actual,options,text);}
module.exports={compare};
