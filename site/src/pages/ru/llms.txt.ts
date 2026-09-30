import type { APIRoute } from 'astro';
import { productSummary } from '../../data/product-summary';

export const GET: APIRoute = ({ site }) =>
  new Response(productSummary('ru', site!), { headers: { 'Content-Type': 'text/plain; charset=utf-8' } });
