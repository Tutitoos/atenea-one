import { renderToReadableStream } from 'react-dom/server';
import { ServerRouter, type EntryContext } from 'react-router';
// Solo se ejecuta al compilar la SPA; no hay servidor Node en producción.
export default async function handleRequest(request: Request, status: number, headers: Headers, context: EntryContext) {
  const body = await renderToReadableStream(<ServerRouter context={context} url={request.url}/>);
  await body.allReady;
  headers.set('Content-Type', 'text/html');
  return new Response(body, { status, headers });
}
