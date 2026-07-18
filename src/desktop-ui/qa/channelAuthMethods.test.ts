import { expect, test } from "bun:test";

import { firstSupportedAuthMethod } from "../src/Onboarding/authMethods";

test("selects the first auth method supported by the desktop", () => {
  expect(firstSupportedAuthMethod(["qrcode_login"])).toBe("qrcode_login");
  expect(firstSupportedAuthMethod(["pairing_code"])).toBe("pairing_code");
  expect(
    firstSupportedAuthMethod(["future_method", "pairing_code", "qrcode_login"]),
  ).toBe("pairing_code");
});

test("returns undefined when the manifest has no supported auth method", () => {
  expect(firstSupportedAuthMethod(undefined)).toBeUndefined();
  expect(firstSupportedAuthMethod([])).toBeUndefined();
  expect(firstSupportedAuthMethod(["future_method"])).toBeUndefined();
});
