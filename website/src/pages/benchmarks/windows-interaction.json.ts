import type { APIRoute } from "astro";
import runs from "../../data/windows-interaction.json";
import machine from "../../data/windows-test-machine.json";

export const GET: APIRoute = () =>
  new Response(JSON.stringify({ machine, unit: "ms", runs }, null, 2) + "\n", {
    headers: { "Content-Type": "application/json; charset=utf-8" },
  });
