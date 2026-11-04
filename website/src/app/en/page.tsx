import { Home, homeMetadata } from "@/views/Home";

export const metadata = homeMetadata("en");

export default function Page() {
  return <Home lang="en" />;
}
