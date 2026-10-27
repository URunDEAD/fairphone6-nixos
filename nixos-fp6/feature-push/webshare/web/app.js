(function () {
  'use strict';
  var status = document.getElementById('status');
  function post(path, body, headers, button) {
    button.disabled = true;
    status.textContent = 'Sending…';
    return fetch(path, { method: 'POST', body: body, headers: headers || {}, credentials: 'same-origin' })
      .then(function (response) { return response.text().then(function (text) { status.textContent = text; return response.ok; }); })
      .catch(function () { status.textContent = 'Connection closed. Check the phone: the share may have ended.'; return false; })
      .then(function (ok) { button.disabled = false; return ok; });
  }
  var upload = document.getElementById('upload');
  if (upload) upload.onclick = function () {
    var input = document.getElementById('file');
    var file = input.files[0];
    if (!file) { status.textContent = 'Choose a file first.'; return; }
    if (file.size > 100 * 1024 * 1024) { status.textContent = 'Maximum file size is 100 MiB.'; return; }
    post('upload', file, { 'X-File-Name': encodeURIComponent(file.name), 'Content-Type': 'application/octet-stream' }, upload)
      .then(function (ok) { if (ok) input.value = ''; });
  };
  var send = document.getElementById('sendText');
  if (send) send.onclick = function () {
    var text = document.getElementById('text');
    post('text', text.value, { 'Content-Type': 'text/plain;charset=UTF-8' }, send)
      .then(function (ok) { if (ok) text.value = ''; });
  };
  document.getElementById('finish').onclick = function () { post('finish', '', {}, this); };
}());
