// SPDX-License-Identifier: MIT OR Apache-2.0
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdtemp, readdir, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright-core';
import { request } from 'node:http';

const root=fileURLToPath(new URL('../../',import.meta.url));
const tmp=await mkdtemp(join(tmpdir(),'vp-dashboard-'));
const binary=resolve(root,'target/debug/vapour-profiler');
const children=[];
async function launch(args=[]){
  const child=spawn(binary,['dashboard','--port','0',...(args.includes('--ttl')?[]:['--ttl','30']),...args],{env:{...process.env,XDG_DATA_HOME:tmp,ANTHROPIC_API_KEY:''},stdio:['ignore','pipe','pipe']});children.push(child);
  let output='';child.stdout.on('data',data=>output+=data);child.stderr.on('data',data=>output+=data);
  const url=await new Promise((resolve,reject)=>{const timeout=setTimeout(()=>reject(Error('Agent startup timeout: '+output)),10000);child.stdout.on('data',()=>{const match=output.match(/http:\/\/127\.0\.0\.1:\d+#token=[a-f0-9]+/);if(match){clearTimeout(timeout);resolve(match[0]);}});child.once('exit',code=>{clearTimeout(timeout);reject(Error('Agent exited '+code+': '+output));});});
  const address=new URL(url), token=address.hash.slice(7), origin=address.origin;
  const api=(path,body,headers={})=>fetch(origin+'/api/'+path,{method:body===undefined?'GET':'POST',headers:{Authorization:'Bearer '+token,...(body===undefined?{}:{'Content-Type':'application/json'}),...headers},body:body===undefined?undefined:typeof body==='string'?body:JSON.stringify(body)});
  return{child,url,origin,token,api,output:()=>output};
}
async function exited(child){if(child.exitCode!==null)return;await new Promise((resolve,reject)=>{const timer=setTimeout(()=>reject(Error('Agent failed to exit')),5000);child.once('exit',()=>{clearTimeout(timer);resolve();});});}
const trace=[{type:'metadata',format:'vpt',version:1,source:'browser',profile:'custom',startedAt:'2026-10-14T00:00:00Z',adapter:'smoke-test'},
  ...[1,2,3].map(i=>({type:'metrics',timeMs:i*1000,values:{fps:60,low1Fps:40,mainThreadBusyPercent:25,wasmBytes:i*65536}})),
  ...Array.from({length:120},(_,i)=>({type:'frame',timeMs:(i+1)*16,durationMs:i===90?80:16,spike:i===90})),
  {type:'memory',kind:'grow',timeMs:2000,fromBytes:65536,toBytes:131072},
  {type:'stack',stack:'main;planted_slow_loop',samples:80},{type:'stack',stack:'main;update',samples:5},
  {type:'end',durationMs:3000,dropped:{}}].map(JSON.stringify).join('\n');
let browser;
try{
  const viewer=await launch();
  assert.equal((await fetch(viewer.origin+'/api/status')).status,401);
  assert.equal((await viewer.api('status',undefined,{Origin:'https://production.example'})).status,403);
  const wrongHost=await new Promise((resolve,reject)=>{const req=request(viewer.origin+'/api/status',{headers:{Host:'attacker.example',Authorization:'Bearer '+viewer.token}},res=>{res.resume();resolve(res.statusCode);});req.on('error',reject);req.end();});
  assert.equal(wrongHost,403);
  assert.equal((await viewer.api('start',{})).status,400);
  assert.equal((await viewer.api('trace','{}')).status,400);
  browser=await chromium.launch({executablePath:process.env.CHROMIUM_PATH??chromium.executablePath(),headless:true});
  const page=await browser.newPage({viewport:{width:1440,height:1100}});
  const errors=[];page.on('pageerror',e=>errors.push(e.message));
  await page.goto(viewer.url);
  await page.waitForFunction(()=>document.querySelector('#status').textContent.includes('Local viewer'));
  assert.equal(new URL(page.url()).hash,'');
  await page.locator('#file').setInputFiles({name:'capture.vpt',mimeType:'application/x-ndjson',buffer:Buffer.from(trace)});
  await page.waitForFunction(()=>document.querySelector('#functions').textContent.includes('planted_slow_loop'));
  assert.equal(await page.locator('#spikes').textContent(),'1');
  await page.locator('#offline').click();
  await page.waitForFunction(()=>document.querySelector('#report').textContent.includes('planted_slow_loop'));
  assert.match(await page.locator('#report').textContent(),/Sustained memory growth/);
  await page.locator('#baseline').click();
  await page.locator('#range-start').fill('20');await page.locator('#range-start').dispatchEvent('input');
  await page.locator('#search').fill('slow');
  await page.screenshot({path:join(root,'target/dashboard.png'),fullPage:true});
  const download=page.waitForEvent('download');await page.locator('#dump').click();assert.equal((await download).suggestedFilename(),'capture.vpt');
  await page.route('**/api/key',route=>route.fulfill({status:200,contentType:'application/json',body:'{"ok":true}'}));
  await page.locator('summary').click();
  await page.locator('#key').fill('fake-test-key');await page.locator('#save-key').click();
  assert.equal(await page.locator('#key').inputValue(),'');
  // Never exercise a real keyring or provider in this smoke test: intercept key saving.
  assert.deepEqual(errors,[]);
  await viewer.api('off',{});await exited(viewer.child);
  await assert.rejects(fetch(viewer.origin+'/api/status'));
  const native=await launch(['--pid',String(process.pid)]);
  assert.equal((await native.api('start',{})).status,200);
  await new Promise(resolve=>setTimeout(resolve,1250));
  const status=await(await native.api('status')).json();assert.equal(status.active,true);assert(status.metrics.values.rssBytes>0);
  await native.api('off',{});await exited(native.child);
  const dumps=await readdir(join(tmp,'vapour-profiler/dumps'));assert.equal(dumps.length,1);
  const saved=(await readFile(join(tmp,'vapour-profiler/dumps',dumps[0]),'utf8')).trim().split('\n').map(JSON.parse);assert.equal(saved.at(-1).type,'end');
  const ttl=await launch(['--ttl','1']);await exited(ttl.child);await assert.rejects(fetch(ttl.origin));
  console.log('PASS: authentication/origin/host checks, Chromium UI, trace import/export, flamegraph, offline ranking/growth, native /proc capture, dump flush, off and TTL cleanup.');
}finally{await browser?.close();for(const child of children)if(child.exitCode===null)child.kill('SIGKILL');await rm(tmp,{recursive:true,force:true});}
