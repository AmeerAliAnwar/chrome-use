// Predicted Automation Engine for chrome-use (MV3 / In-Page / Extension).
// Executes verified sequences at native machine speed, saving tokens and round-trips.
// If page structure or triggers deviate from expectations, halts immediately and
// yields control back to the AI.

/**
 * Evaluates triggers and anti-triggers on a document.
 * Returns { diverged: boolean, divergenceType?: string, reason?: string, selector?: string }
 */
export function evaluateAutomationTriggers(
  preconditions = [],
  antiTriggers = [],
  doc = typeof document !== 'undefined' ? document : null
) {
  if (!doc) {
    return {
      diverged: true,
      divergenceType: 'document_unavailable',
      reason: 'Document context is not available',
    };
  }

  // 1. Anti-trigger check (divergence indicators: captchas, error messages, rate limits, modals)
  for (const at of antiTriggers) {
    if (!at?.selector) continue;
    try {
      const el = doc.querySelector(at.selector);
      if (el) {
        // Element exists, check if visible
        const rects = typeof el.getClientRects === 'function' ? el.getClientRects() : null;
        const isVisible = el.offsetWidth > 0 || el.offsetHeight > 0 || (rects && rects.length > 0);
        if (isVisible) {
          return {
            diverged: true,
            divergenceType: 'anti_trigger',
            reason: at.description || `Anti-trigger detected: ${at.selector}`,
            selector: at.selector,
            matchedHtml: el.outerHTML ? el.outerHTML.slice(0, 200) : '',
          };
        }
      }
    } catch (err) {
      // Invalid selector or DOM error
      return {
        diverged: true,
        divergenceType: 'selector_syntax_error',
        reason: `Invalid anti-trigger selector: ${at.selector} (${err.message})`,
        selector: at.selector,
      };
    }
  }

  // 2. Preconditions check (required elements, text, visibility)
  for (const pre of preconditions) {
    if (!pre?.selector) continue;
    try {
      const el = doc.querySelector(pre.selector);
      if (!el) {
        return {
          diverged: true,
          divergenceType: 'precondition_missing',
          reason: `Required precondition selector not found: ${pre.selector}`,
          selector: pre.selector,
        };
      }

      if (pre.visible) {
        const rects = typeof el.getClientRects === 'function' ? el.getClientRects() : null;
        const isVisible = el.offsetWidth > 0 || el.offsetHeight > 0 || (rects && rects.length > 0);
        if (!isVisible) {
          return {
            diverged: true,
            divergenceType: 'precondition_hidden',
            reason: `Precondition element is hidden: ${pre.selector}`,
            selector: pre.selector,
          };
        }
      }

      if (typeof pre.text === 'string' && pre.text.length > 0) {
        const text = (el.innerText || el.textContent || '').trim();
        if (!text.includes(pre.text)) {
          return {
            diverged: true,
            divergenceType: 'precondition_text_mismatch',
            reason: `Precondition text mismatch on ${pre.selector}: expected '${pre.text}', got '${text}'`,
            selector: pre.selector,
            actualText: text,
          };
        }
      }
    } catch (err) {
      return {
        diverged: true,
        divergenceType: 'precondition_error',
        reason: `Error checking precondition: ${pre.selector} (${err.message})`,
        selector: pre.selector,
      };
    }
  }

  return { diverged: false };
}

/**
 * Execute a single action step on the DOM.
 * Supports React controlled inputs, synthetic event dispatch, clicks, waits.
 */
export async function executeAutomationStep(
  step,
  doc = typeof document !== 'undefined' ? document : null,
  win = typeof window !== 'undefined' ? window : null
) {
  if (!step || !step.action) {
    throw new Error('Invalid step configuration');
  }

  const { action, selector, value, waitMs } = step;

  if (action === 'wait') {
    const delay =
      typeof waitMs === 'number' ? waitMs : typeof value === 'number' ? value : 100;
    await new Promise((resolve) => setTimeout(resolve, delay));
    return { success: true, action: 'wait', waitMs: delay };
  }

  if (!doc) {
    throw new Error('Document context unavailable');
  }

  const el = doc.querySelector(selector);
  if (!el) {
    throw new Error(`Element not found for selector: ${selector}`);
  }

  const dispatchCustom = (name, Cls, init = {}) => {
    if (typeof Cls !== 'undefined') {
      el.dispatchEvent(new Cls(name, { bubbles: true, cancelable: true, ...init }));
    } else {
      el.dispatchEvent({ type: name, ...init });
    }
  };

  switch (action) {
    case 'click': {
      el.scrollIntoView?.({ block: 'nearest', inline: 'nearest' });
      el.focus?.();
      dispatchCustom(
        'mousedown',
        typeof MouseEvent !== 'undefined' ? MouseEvent : undefined,
        win ? { view: win } : {}
      );
      dispatchCustom(
        'mouseup',
        typeof MouseEvent !== 'undefined' ? MouseEvent : undefined,
        win ? { view: win } : {}
      );
      el.click();
      return { success: true, action: 'click', selector };
    }

    case 'fill':
    case 'input': {
      el.scrollIntoView?.({ block: 'nearest', inline: 'nearest' });
      el.focus?.();

      const isInput =
        (typeof HTMLInputElement !== 'undefined' && el instanceof HTMLInputElement) ||
        el.tagName === 'INPUT';
      const isTextArea =
        (typeof HTMLTextAreaElement !== 'undefined' && el instanceof HTMLTextAreaElement) ||
        el.tagName === 'TEXTAREA';

      const proto = isTextArea
        ? typeof HTMLTextAreaElement !== 'undefined'
          ? HTMLTextAreaElement.prototype
          : null
        : isInput
        ? typeof HTMLInputElement !== 'undefined'
          ? HTMLInputElement.prototype
          : null
        : null;

      const descriptor = proto ? Object.getOwnPropertyDescriptor(proto, 'value') : null;
      const targetValue = typeof value === 'string' ? value : String(value ?? '');

      if (descriptor && descriptor.set) {
        descriptor.set.call(el, targetValue);
      } else {
        el.value = targetValue;
      }

      dispatchCustom('input', typeof Event !== 'undefined' ? Event : undefined);
      dispatchCustom('change', typeof Event !== 'undefined' ? Event : undefined);
      return { success: true, action, selector, value: targetValue };
    }

    case 'select': {
      const isSelect =
        (typeof HTMLSelectElement !== 'undefined' && el instanceof HTMLSelectElement) ||
        el.tagName === 'SELECT';
      if (isSelect) {
        el.value = String(value ?? '');
        dispatchCustom('change', typeof Event !== 'undefined' ? Event : undefined);
        return { success: true, action: 'select', selector, value };
      }
      throw new Error(`Element ${selector} is not a select element`);
    }

    case 'press': {
      const key = String(value || 'Enter');
      dispatchCustom('keydown', typeof KeyboardEvent !== 'undefined' ? KeyboardEvent : undefined, {
        key,
      });
      dispatchCustom('keyup', typeof KeyboardEvent !== 'undefined' ? KeyboardEvent : undefined, {
        key,
      });
      return { success: true, action: 'press', selector, key };
    }

    default:
      throw new Error(`Unsupported automation action: ${action}`);
  }
}

/**
 * Execute a full predicted automation pipeline with deviation guards.
 */
export async function executePredictedAutomation(
  automation,
  doc = typeof document !== 'undefined' ? document : null,
  win = typeof window !== 'undefined' ? window : null
) {
  if (!automation) {
    return {
      success: false,
      diverged: true,
      divergenceType: 'invalid_automation',
      reason: 'No automation specification provided',
    };
  }

  const { id, preconditions = [], antiTriggers = [], steps = [], postConditionSelector } =
    automation;

  // 1. Initial trigger evaluation
  const triggerEval = evaluateAutomationTriggers(preconditions, antiTriggers, doc);
  if (triggerEval.diverged) {
    return {
      success: false,
      diverged: true,
      automationId: id,
      ...triggerEval,
      message: `Predicted automation '${id}' diverged at trigger check: ${triggerEval.reason}. Control yielded to AI.`,
    };
  }

  // 2. Step execution pipeline
  let stepIndex = 0;
  for (const step of steps) {
    try {
      await executeAutomationStep(step, doc, win);
      stepIndex++;
    } catch (err) {
      return {
        success: false,
        diverged: true,
        automationId: id,
        divergenceType: 'step_execution_failure',
        stepIndex,
        stepAction: step?.action,
        stepSelector: step?.selector,
        reason: err.message,
        message: `Predicted automation '${id}' diverged during execution at step ${stepIndex}: ${err.message}. Control yielded to AI.`,
      };
    }
  }

  // 3. Post-condition check (if requested)
  if (postConditionSelector) {
    const postEl = doc.querySelector(postConditionSelector);
    if (!postEl) {
      return {
        success: false,
        diverged: true,
        automationId: id,
        divergenceType: 'post_condition_missing',
        selector: postConditionSelector,
        reason: `Post-condition selector not found: ${postConditionSelector}`,
        message: `Predicted automation '${id}' completed steps but post-condition was not satisfied. Control yielded to AI.`,
      };
    }
  }

  const tokensSaved = steps.length * 500;
  return {
    success: true,
    diverged: false,
    automationId: id,
    stepsExecuted: steps.length,
    tokensSaved,
    message: `Predicted automation '${id}' executed successfully in 1 atomic stroke. Saved ~${tokensSaved} tokens.`,
  };
}
