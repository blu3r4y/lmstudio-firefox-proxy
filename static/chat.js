(function() {
  var statusEl   = document.getElementById('status');
  var responseEl = document.getElementById('response');
  var errorEl    = document.getElementById('error');
  var thinkWrap  = document.getElementById('thinking-wrapper');
  var thinkHead  = document.getElementById('thinking-header');
  var thinkEl    = document.getElementById('thinking');
  var thinkLabel = thinkHead.querySelector('.label');
  var thinkChev  = thinkHead.querySelector('.chevron');

  var rawContent = '', isDone = false, renderFrame = null;
  var thinkingDone = false;

  // Toggle thinking visibility on header click
  thinkHead.addEventListener('click', function() {
    thinkEl.classList.toggle('collapsed');
    thinkChev.classList.toggle('collapsed');
  });

  function parseThinking() {
    var startTag = rawContent.indexOf('<think>');
    if (startTag === -1) return { thinking: '', main: rawContent, isThinking: false };
    var afterStart = startTag + 7;
    var endTag = rawContent.indexOf('</think>', afterStart);
    if (endTag === -1) {
      // Still inside thinking block
      return { thinking: rawContent.substring(afterStart), main: '', isThinking: true };
    }
    // Thinking complete
    return {
      thinking: rawContent.substring(afterStart, endTag),
      main: rawContent.substring(endTag + 8),
      isThinking: false
    };
  }

  function renderAll() {
    var parsed = parseThinking();
    var hasThinking = parsed.thinking.length > 0;

    if (hasThinking) {
      thinkWrap.classList.add('active');
      thinkEl.innerHTML = marked.parse(parsed.thinking.trim());

      if (!parsed.isThinking && !thinkingDone) {
        // Thinking just finished — collapse and relabel
        thinkingDone = true;
        thinkLabel.textContent = 'Show reasoning';
        thinkEl.classList.add('collapsed');
        thinkChev.classList.add('collapsed');
      }

      // Auto-scroll thinking div while still streaming thought
      if (parsed.isThinking) {
        thinkEl.scrollTop = thinkEl.scrollHeight;
      }
    }

    var mainSource = hasThinking ? parsed.main : rawContent;
    if (mainSource && (!parsed.isThinking || !hasThinking)) {
      responseEl.innerHTML = marked.parse(mainSource.trim());
      responseEl.querySelectorAll('pre code:not(.hljs)').forEach(function(el) {
        hljs.highlightElement(el);
      });
    }
  }

  function scheduleRender() {
    if (renderFrame) return;
    renderFrame = requestAnimationFrame(function() {
      renderFrame = null;
      renderAll();
      window.scrollTo(0, document.body.scrollHeight);
    });
  }

  function showError(msg) {
    statusEl.style.display = 'none';
    responseEl.classList.remove('streaming');
    errorEl.style.display = 'block';
    errorEl.textContent = msg;
  }

  var es = new EventSource('/api/chat' + window.location.search);

  es.onmessage = function(e) {
    statusEl.style.display = 'none';
    rawContent += e.data;
    scheduleRender();
  };

  es.addEventListener('done', function() {
    isDone = true;
    es.close();
    responseEl.classList.remove('streaming');
    renderAll();
  });

  es.addEventListener('server_error', function(e) {
    isDone = true;
    es.close();
    showError(e.data);
  });

  es.onerror = function() {
    es.close();
    responseEl.classList.remove('streaming');
    if (!isDone && !rawContent) {
      showError('Failed to connect to the proxy server.');
    } else if (rawContent) {
      renderAll();
    }
  };
})();
