(function() {
  var statusEl   = document.getElementById('status');
  var responseEl = document.getElementById('response');
  var errorEl    = document.getElementById('error');
  var followupEl = document.getElementById('followup');
  var followupIn = document.getElementById('followup-input');
  var followupGo = document.getElementById('followup-send');
  var thinkWrap  = document.getElementById('thinking-wrapper');
  var thinkHead  = document.getElementById('thinking-header');
  var thinkEl    = document.getElementById('thinking');
  var thinkLabel = thinkHead.querySelector('.label');
  var thinkChev  = thinkHead.querySelector('.chevron');

  var rawContent = '', isDone = false, renderFrame = null;
  var thinkingDone = false;
  var es = null;
  var currentRequestId = 0;
  var currentQuestion = '';
  var initialPrompt = new URLSearchParams(window.location.search).get('q') || '';
  var turns = [];

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

  function plainAnswerText() {
    var parsed = parseThinking();
    return (parsed.main || rawContent).replace(/<\/?think>/g, '').trim();
  }

  function resetForStream() {
    rawContent = '';
    isDone = false;
    thinkingDone = false;
    if (renderFrame) {
      cancelAnimationFrame(renderFrame);
      renderFrame = null;
    }

    statusEl.style.display = 'flex';
    errorEl.style.display = 'none';
    errorEl.textContent = '';

    responseEl.classList.add('streaming');
    responseEl.innerHTML = '';

    thinkWrap.classList.remove('active');
    thinkLabel.textContent = 'Thinking\u2026';
    thinkChev.classList.remove('collapsed');
    thinkEl.classList.remove('collapsed');
    thinkEl.innerHTML = '';
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
    followupEl.classList.add('active');
    followupGo.disabled = false;
  }

  function streamPrompt(prompt, userQuestion) {
    if (!prompt) {
      showError('Missing prompt.');
      return;
    }

    currentRequestId += 1;
    var requestId = currentRequestId;
    currentQuestion = userQuestion || '';

    if (es) {
      es.close();
      es = null;
    }

    followupEl.classList.remove('active');
    followupGo.disabled = true;
    resetForStream();

    es = new EventSource('/api/chat?q=' + encodeURIComponent(prompt));

    es.onmessage = function(e) {
      if (requestId !== currentRequestId) return;
      statusEl.style.display = 'none';
      rawContent += e.data;
      scheduleRender();
    };

    es.addEventListener('done', function() {
      if (requestId !== currentRequestId) return;
      isDone = true;
      es.close();
      es = null;
      responseEl.classList.remove('streaming');
      renderAll();

      var answer = plainAnswerText();
      if (currentQuestion && answer) {
        turns.push({ question: currentQuestion, answer: answer });
      }

      followupEl.classList.add('active');
      followupGo.disabled = false;
      followupIn.focus();
    });

    es.addEventListener('server_error', function(e) {
      if (requestId !== currentRequestId) return;
      isDone = true;
      es.close();
      es = null;
      showError(e.data);
    });

    es.onerror = function() {
      if (requestId !== currentRequestId) return;
      es.close();
      es = null;
      responseEl.classList.remove('streaming');
      if (!isDone && !rawContent) {
        showError('Failed to connect to the proxy server.');
      } else if (rawContent) {
        renderAll();
        followupEl.classList.add('active');
        followupGo.disabled = false;
      }
    };
  }

  function composeFollowupPrompt(question) {
    var segments = [
      'You are continuing a conversation about previously provided content.'
    ];

    if (initialPrompt.trim()) {
      segments.push('Initial content/request:\n' + initialPrompt.trim());
    }

    if (turns.length) {
      var recent = turns.slice(-3).map(function(turn, i) {
        var n = i + 1;
        return 'Recent turn ' + n + ' user:\n' + turn.question +
          '\n\nRecent turn ' + n + ' assistant:\n' + turn.answer;
      }).join('\n\n');
      segments.push(recent);
    }

    segments.push('Follow-up question:\n' + question);
    return segments.join('\n\n');
  }

  followupEl.addEventListener('submit', function(e) {
    e.preventDefault();
    var question = followupIn.value.trim();
    if (!question || followupGo.disabled) return;
    followupIn.value = '';
    var prompt = composeFollowupPrompt(question);
    streamPrompt(prompt, question);
  });

  streamPrompt(initialPrompt, initialPrompt);
})();
