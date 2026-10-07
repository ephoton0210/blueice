type Shape = {value: number};
let service: {use: (value: Shape) => void} = {use(value: Shape): void {}};
const source = {value: 2, extra: true};
service.use(source);
