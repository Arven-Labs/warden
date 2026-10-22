import type { Metadata } from "next";
import { DocView } from "../../../components/DocView.tsx";
import { DOC_SLUGS, loadDoc } from "../../../lib/docs.ts";

type Params = { slug: string };

export function generateStaticParams(): Params[] {
  return DOC_SLUGS.map((slug) => ({ slug }));
}

export async function generateMetadata({ params }: { params: Promise<Params> }): Promise<Metadata> {
  const { slug } = await params;
  return { title: loadDoc(slug).title };
}

export default async function DocPage({ params }: { params: Promise<Params> }) {
  const { slug } = await params;
  return <DocView slug={slug} />;
}
