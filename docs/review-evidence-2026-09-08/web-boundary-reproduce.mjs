import fs from 'node:fs';
import vm from 'node:vm';
import assert from 'node:assert/strict';
import { stripTypeScriptTypes } from 'node:module';
const root = '/Users/nyabi/Documents/Code/Ugurugu';
const read = relative => fs.readFileSync(`${root}/${relative}`, 'utf8');
const app = read('web/src/App.svelte');
function appFunction(name) {
  let begin = app.indexOf(`    function ${name}(`);
  if (begin < 0) begin = app.indexOf(`    async function ${name}(`);
  assert.ok(begin >= 0);
  const end = app.indexOf('\n    }', begin) + 6;
  return stripTypeScriptTypes(app.slice(begin, end));
}

// Execute actual enqueue/downloadDocument functions. Mock the slow request and
// engine serialization, not the shell queue. No browser or WASM claims made.
const trace=[];
let persisted=0;
let release;
const blocking=new Promise(resolve=>release=resolve);
const ctx={ Blob, documentName:'Untitled.ugu', status:'', describe:String,
  engine:{ async serialize(){trace.push(`serialize committed=${persisted}`);return Uint8Array.of(persisted).buffer;} },
  downloadBlob:async blob=>trace.push(`downloaded value=${new Uint8Array(await blob.arrayBuffer())[0]}`),
  blocking, commit:()=>{persisted=1;trace.push('strokeEnd committed=1');}
};
vm.createContext(ctx);
vm.runInContext('let chain=Promise.resolve();\n'+appFunction('enqueue')+'\n'+appFunction('downloadDocument')+'\nenqueue(async()=>await blocking); enqueue(async()=>commit());',ctx);
await vm.runInContext('downloadDocument()',ctx);
release();
await vm.runInContext('chain',ctx);
await Promise.resolve();
assert.deepEqual(trace,['serialize committed=0','downloaded value=0','strokeEnd committed=1']);
console.log('QUEUE_SAVE_REPRO:',JSON.stringify(trace));

// Execute actual controller. Delay IDB completion to reproduce an old save
// finishing after adoption of another document has reset its revision marker.
let source=read('web/src/lib/AutosaveController.svelte.ts').replace(/^import[\s\S]*?from "\.\/RecoveryStore";\n/gm,'').replace('export class AutosaveController','class AutosaveController').replace('export function autosaveIntervalMs','function autosaveIntervalMs');
source=stripTypeScriptTypes(source)+'\nglobalThis.Controller=AutosaveController;';
let revision=1,name='A.ugu',releaseWrite;
const records=[];
const autosaveCtx={$state:value=>value,Date,Error,String,
  writeRecoverySnapshot:record=>new Promise(resolve=>{records.push({name:record.name,byte:new Uint8Array(record.bytes)[0]});releaseWrite=resolve;}),
  readRecoverySnapshot:async()=>null,clearRecoverySnapshot:async()=>{}
};
vm.createContext(autosaveCtx);vm.runInContext(source,autosaveCtx);
const controller=new autosaveCtx.Controller({ready:()=>true,revision:()=>revision,name:()=>name,serialize:async()=>Uint8Array.of(name==='A.ugu'?65:66).buffer,open:()=>{}});
const first=controller.snapshot();
await new Promise(setImmediate);
assert.equal(records.length,1);
name='B.ugu';revision=0;controller.reset();
releaseWrite();await first;
revision=1;
await controller.snapshot();
assert.deepEqual(records,[{name:'A.ugu',byte:65}]);
console.log('AUTOSAVE_DOCUMENT_SWAP_REPRO:',JSON.stringify({records,newDocumentName:name,newDocumentRevision:revision,writeCount:records.length,expectedWrites:2}));

// Execute actual initial preference expression with storage denied.
const prefStart=app.indexOf('    let animateWhileDrawing = $state(');
const prefEnd=app.indexOf('\n    );',prefStart)+7;
const prefExpr=app.slice(prefStart,prefEnd);
const blockedCtx={$state:value=>value,window:{get localStorage(){throw new DOMException('Access denied','SecurityError');}}};
vm.createContext(blockedCtx);
assert.throws(()=>vm.runInContext(prefExpr,blockedCtx),{name:'SecurityError'});
console.log('STORAGE_INITIALIZATION_REPRO: actual initial preference expression throws uncaught SecurityError');

// Execute the exact shortcut module with minimal DOM target classes.
let shortcut=read('web/src/lib/Shortcuts.ts').replace(/^import type .*;\n/m,'');
shortcut=stripTypeScriptTypes(shortcut).replace('export function handleShortcut','function handleShortcut')+'\nglobalThis.shortcut=handleShortcut;';
class FakeInput {constructor(type){this.type=type;this.tagName='INPUT';this.isContentEditable=false;}}
const shortcutCtx={HTMLInputElement:FakeInput};vm.createContext(shortcutCtx);vm.runInContext(shortcut,shortcutCtx);
const events=[];
const actions={stepFrame:delta=>events.push(`stepFrame(${delta})`),togglePlayback:()=>events.push('togglePlayback')};
const handledRange=shortcutCtx.shortcut({target:new FakeInput('range'),key:'ArrowRight',ctrlKey:false,metaKey:false,altKey:false},actions);
const handledButton=shortcutCtx.shortcut({target:{tagName:'BUTTON',isContentEditable:false},key:'Enter',ctrlKey:false,metaKey:false,altKey:false},actions);
assert.equal(handledRange,true);assert.equal(handledButton,true);
assert.deepEqual(events,['stepFrame(1)','togglePlayback']);
console.log('KEYBOARD_CONTROL_REPRO:',JSON.stringify({events,handledRange,handledButton,meaning:'App prevents default for both'}));
