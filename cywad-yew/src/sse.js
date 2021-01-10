export function registerSseHandler(url, callback) {

    const eventSource = new EventSource(url, {
        withCredentials: true,
    });

    eventSource.addEventListener(
        'heartbeat',
        e => {
            callback('heartbeat', e.data);
        },
        false
    );

    eventSource.addEventListener(
        'item',
        e => {
            callback('item', e.data);
        },
        false
    );
}
