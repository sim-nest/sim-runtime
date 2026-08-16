let packet = JSON.parse('{"10":"ten","2":"two","drop":true}', (key, value) =>
  key === "drop" ? undefined : value
);
JSON.stringify(packet);
