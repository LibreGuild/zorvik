import { expect, test, type Page } from "@playwright/test";
import fs from "node:fs";
import path from "node:path";

// The testkit's gRPC echo server (`cargo run -p zorvik-testkit -- --port 18787` → gRPC on PORT+3).
const GRPC = "grpc://127.0.0.1:18790";
const PROTOS = path.resolve(process.cwd(), "../crates/testkit/proto");

async function newGrpc(page: Page, url = GRPC) {
  await page.getByRole("button", { name: "New tab" }).click();
  await page.getByRole("menuitem", { name: "gRPC request" }).click();
  await page.getByLabel("URL", { exact: true }).fill(url);
}

async function pickMethod(page: Page, name: string) {
  await page.getByRole("button", { name: "gRPC method" }).click();
  await page.getByRole("option", { name: new RegExp(`^${name}\\b`) }).click();
}

/** Replace the message editor's text (no key events: no auto-closed brackets). */
async function setMessage(page: Page, text: string) {
  await page.getByTestId("grpc-message-editor").locator(".cm-content").click();
  await page.keyboard.press("ControlOrMeta+A");
  await page.keyboard.insertText(text);
}

test.beforeEach(async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("workspace-menu")).toContainText("E2E Workspace");
});

test("gRPC: reflection lists the methods and a unary call answers", async ({ page }) => {
  await newGrpc(page);
  await page.getByRole("button", { name: "gRPC method" }).click();
  const methods = page.getByTestId("grpc-methods");
  await expect(methods).toContainText("zorvik.test.v1.Echo");
  await expect(methods).toContainText("bidi");
  await page.getByLabel("Search methods").fill("unary");
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("grpc-method")).toHaveText("Echo/Unary");
  // Picking a method fills in its example message.
  await expect(page.getByTestId("grpc-message-editor")).toContainText('"mood": "MOOD_UNSPECIFIED"');

  await setMessage(page, '{"message": "hello e2e", "mood": "HAPPY"}');
  await page.getByRole("button", { name: "Send", exact: true }).click();
  await expect(page.getByTestId("grpc-status")).toHaveText("0 OK");
  await expect(page.getByTestId("grpc-result")).toContainText("hello e2e");
  await page.getByRole("tab", { name: /^Trailers/ }).click();
  await expect(page.getByTestId("grpc-result")).toContainText("grpc-status");
  await page.getByRole("tab", { name: "Timing" }).click();
  await expect(page.getByText("DNS lookup")).toBeVisible();
});

test("gRPC: an error shows its status, message and details", async ({ page }) => {
  await newGrpc(page);
  await pickMethod(page, "Fail");
  await setMessage(page, '{"code": 7, "message": "not for you"}');
  await page.keyboard.press("ControlOrMeta+Enter");
  await expect(page.getByTestId("grpc-status")).toHaveText("7 PERMISSION_DENIED");
  const panel = page.getByTestId("grpc-status-panel");
  await expect(panel).toContainText("not for you");
  await expect(panel).toContainText("zorvik.test.v1.FailRequest");
});

test("gRPC: server stream and bidi stream", async ({ page }) => {
  await newGrpc(page);
  await pickMethod(page, "ServerStream");
  await setMessage(page, '{"message": "tick", "count": 3}');
  await page.getByRole("button", { name: "Send", exact: true }).click();
  await expect(page.getByTestId("grpc-status")).toHaveText("0 OK");
  await expect(page.getByTestId("grpc-message")).toHaveCount(4);
  await expect(page.getByTestId("grpc-messages")).toContainText("tick #3");

  await pickMethod(page, "Bidi");
  await page.getByRole("button", { name: "Send", exact: true }).click();
  await expect(page.getByTestId("grpc-stream-state")).toHaveText("Stream open");
  await setMessage(page, '{"message": "ping"}');
  await page.getByRole("button", { name: "Send message" }).click();
  await expect(page.getByTestId("grpc-message")).toHaveCount(2);
  await page.getByRole("button", { name: "End stream" }).first().click();
  await expect(page.getByTestId("grpc-status")).toHaveText("0 OK");
});

test("gRPC: services from .proto files in the workspace", async ({ page }) => {
  const workspace = process.env.ZV_E2E_WORKSPACE!;
  const target = path.join(workspace, "protos");
  fs.cpSync(PROTOS, target, { recursive: true });

  await newGrpc(page);
  await page.getByRole("tab", { name: "Proto files" }).click();
  // The browser build asks for the path with a prompt (the desktop app opens a file dialog).
  page.once("dialog", (d) => void d.accept(path.join(target, "zorvik/test/v1/echo.proto")));
  await page.getByRole("button", { name: "Add .proto file…" }).click();
  await expect(page.getByTestId("grpc-proto-files")).toContainText("echo.proto");
  page.once("dialog", (d) => void d.accept(target));
  await page.getByRole("button", { name: "Add folder…" }).click();
  await page.getByRole("button", { name: "Load services from the files" }).click();
  await expect(page.getByTestId("grpc-services-status")).toContainText("1 service, 6 methods from proto files");

  await pickMethod(page, "Unary");
  await setMessage(page, '{"message": "from protos"}');
  await page.getByRole("button", { name: "Send", exact: true }).click();
  await expect(page.getByTestId("grpc-status")).toHaveText("0 OK");
  await expect(page.getByTestId("grpc-result")).toContainText("from protos");
});
