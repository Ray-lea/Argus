import Replica from "@/components/replica/Replica";
import { Home, homeMetadata } from "@/views/Home";
import "@/styles/home-route.css";

export const metadata = homeMetadata("de");

export default function Page() {
  return <Home lang="de" demo={<Replica lang="de" />} />;
}
