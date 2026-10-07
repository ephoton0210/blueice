type Shape = {value: number};
let holder: {item: Shape} = {item: {value: 1}};
const source = {value: 2, extra: true};
holder.item = source;
