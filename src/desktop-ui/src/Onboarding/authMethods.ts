export type ChannelAuthMethod = "qrcode_login" | "pairing_code";

export function firstSupportedAuthMethod(
  methods: string[] | undefined,
): ChannelAuthMethod | undefined {
  return methods?.find(
    (method): method is ChannelAuthMethod =>
      method === "qrcode_login" || method === "pairing_code",
  );
}
