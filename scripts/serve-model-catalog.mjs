import http from 'node:http';
import { readFile } from 'node:fs/promises';

const file = new URL('../model-catalog/catalog.json', import.meta.url);
const server = http.createServer(async (req, res) => {
  if (new URL(req.url, 'http://localhost').pathname !== '/catalog.json') {
    res.writeHead(404).end('Not found');
    return;
  }
  if (req.method !== 'GET' && req.method !== 'HEAD') {
    res.writeHead(405, { Allow: 'GET, HEAD' }).end();
    return;
  }
  try {
    // Read each time so a maintainer can edit the JSON without restarting.
    const body = await readFile(file);
    JSON.parse(body.toString('utf8'));
    res.writeHead(200, {
      'Content-Type': 'application/json; charset=utf-8',
      'Content-Length': body.length,
      'Cache-Control': 'no-cache',
    });
    res.end(req.method === 'HEAD' ? undefined : body);
  } catch (error) {
    console.error(error.message);
    res.writeHead(500).end('Catalog unavailable');
  }
});
server.listen(18081, '127.0.0.1', () => console.log('Model catalog: http://127.0.0.1:18081/catalog.json'));
server.on('error', (error) => { console.error(error.message); process.exitCode = 1; });
