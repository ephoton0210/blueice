type Shape = {value: number};
let service: {use: (value: Shape) => void} = {use(value: Shape): void {}};
service.use({value: 2, extra: true});
