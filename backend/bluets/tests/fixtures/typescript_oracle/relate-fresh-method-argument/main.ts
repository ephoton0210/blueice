type Shape = {value: number};
interface Service {use(value: Shape): void;}
let service: Service;
service = {use(value: Shape): void {}};
service.use({value: 2, extra: true});
