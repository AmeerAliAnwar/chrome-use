// Configure only Chrome's packaged extension surfaces. No page script is used.
export async function configureAgentPanel(chromeApi) {
  try {
    if (!chromeApi.sidePanel?.setPanelBehavior || !chromeApi.sidePanel?.getPanelBehavior) {
      throw new Error('This Chrome version cannot open the agent side panel.');
    }
    await chromeApi.sidePanel.setPanelBehavior({ openPanelOnActionClick: true });
    const behavior = await chromeApi.sidePanel.getPanelBehavior();
    if (behavior?.openPanelOnActionClick !== true) throw new Error('Chrome did not confirm the side panel entry.');
    await chromeApi.action.setPopup({ popup: '' });
    return { ready: true, error: null };
  } catch (failure) {
    let error = String(failure?.message || failure);
    try { await chromeApi.action.setPopup({ popup: 'popup.html' }); }
    catch (fallback) { error += ` Connection popup unavailable: ${fallback?.message || fallback}`; }
    return { ready: false, error };
  }
}
