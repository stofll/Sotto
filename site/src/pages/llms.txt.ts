import type { APIRoute } from 'astro';
import { productSummary } from '../data/product-summary';

export const GET: APIRoute = ({ site }) =>
  new Response(productSummary('en', site!), { headers: { 'Content-Type': 'text/plain; charset=utf-8' } });
