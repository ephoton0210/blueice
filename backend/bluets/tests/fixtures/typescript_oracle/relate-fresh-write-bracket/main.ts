type Shape = {value: number};
let holder: {item: Shape} = {item: {value: 1}};
holder["item"] = {value: 2, extra: true};
