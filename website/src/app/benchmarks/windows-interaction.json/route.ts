import runs from "@/data/windows-interaction.json";
import machine from "@/data/windows-test-machine.json";

export const dynamic = "force-static";
export const revalidate = false;

// The measurements behind the home page's latency board, exported as a file.
export function GET() {
  return new Response(JSON.stringify({ machine, unit: "ms", runs }, null, 2) + "\n", {
    headers: { "Content-Type": "application/json; charset=utf-8" },
  });
}
