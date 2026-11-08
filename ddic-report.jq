#!/usr/bin/env -S jq -f
{
    "apps":[(.apps[]|.name)],
    "entities":[
            .entities[]
            | [ .name,
                (if .draft_enabled == false then "-" else "drafts" end),
                (.seed|length), "seeds",
                (.fields|length), "fields"
            ]
            | map(tostring)|join(" ")
        ],
    "packages": [.packages[]|.name],
    "relationships": [.relationships[]|([.name, ":", .many, "->",.one]|join(" "))],
    "value_lists": [.value_lists[]|[.name, (.values|length), "entries"]|map(tostring)|join(" ")]
}
