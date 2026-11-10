import { policies, type Policy as PolicyName } from "@/lib/site";
import { Policy, policyMetadata } from "@/views/Policy";

interface Props {
  params: Promise<{ policy: string }>;
}

export const dynamicParams = false;
export const generateStaticParams = () => policies.map((policy) => ({ policy }));

export async function generateMetadata({ params }: Props) {
  return policyMetadata("ru", (await params).policy as PolicyName);
}

export default async function Page({ params }: Props) {
  return <Policy lang="ru" policy={(await params).policy as PolicyName} />;
}
