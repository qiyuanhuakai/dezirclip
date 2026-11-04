import { shellCall } from "./bridge";

export const getVersion = () => shellCall<string>("Version");
