use sim_kernel::{Expr, NumberLiteral, Symbol};

pub(crate) fn normalize_lua_codec_expr(expr: Expr) -> Expr {
    match expr {
        Expr::List(items) => Expr::List(items.into_iter().map(normalize_lua_codec_expr).collect()),
        Expr::Vector(items) => {
            Expr::Vector(items.into_iter().map(normalize_lua_codec_expr).collect())
        }
        Expr::Map(entries) => Expr::Map(
            entries
                .into_iter()
                .map(|(key, value)| {
                    (
                        normalize_lua_codec_expr(key),
                        normalize_lua_codec_expr(value),
                    )
                })
                .collect(),
        ),
        Expr::Set(items) => Expr::Set(items.into_iter().map(normalize_lua_codec_expr).collect()),
        Expr::Block(items) => {
            Expr::Block(items.into_iter().map(normalize_lua_codec_expr).collect())
        }
        Expr::Call { operator, args } => normalize_lua_call(*operator, args),
        Expr::Infix {
            operator,
            left,
            right,
        } => Expr::Infix {
            operator,
            left: Box::new(normalize_lua_codec_expr(*left)),
            right: Box::new(normalize_lua_codec_expr(*right)),
        },
        Expr::Prefix { operator, arg } => Expr::Prefix {
            operator,
            arg: Box::new(normalize_lua_codec_expr(*arg)),
        },
        Expr::Postfix { operator, arg } => Expr::Postfix {
            operator,
            arg: Box::new(normalize_lua_codec_expr(*arg)),
        },
        Expr::Quote { mode, expr } => Expr::Quote {
            mode,
            expr: Box::new(normalize_lua_codec_expr(*expr)),
        },
        Expr::Annotated { expr, annotations } => Expr::Annotated {
            expr: Box::new(normalize_lua_codec_expr(*expr)),
            annotations: annotations
                .into_iter()
                .map(|(name, value)| (name, normalize_lua_codec_expr(value)))
                .collect(),
        },
        Expr::Extension { tag, payload } => Expr::Extension {
            tag,
            payload: Box::new(normalize_lua_codec_expr(*payload)),
        },
        other => other,
    }
}

fn normalize_lua_call(operator: Expr, args: Vec<Expr>) -> Expr {
    let operator = normalize_lua_codec_expr(operator);
    let args = args
        .into_iter()
        .map(normalize_lua_codec_expr)
        .collect::<Vec<_>>();
    match operator {
        Expr::Symbol(symbol) if symbol.namespace.as_deref() == Some("lua") => {
            normalize_lua_form_call(symbol, args)
        }
        operator => Expr::Call {
            operator: Box::new(operator),
            args,
        },
    }
}

fn normalize_lua_form_call(symbol: Symbol, args: Vec<Expr>) -> Expr {
    let name = match symbol.name.as_ref() {
        "bit-and" => "band",
        "bit-or" => "bor",
        "bit-xor" => "bxor",
        "floor-div" => "floordiv",
        "for-range" => "for-num",
        "index" => "get",
        other => other,
    };
    if name == "expr" && args.len() == 1 {
        return args.into_iter().next().unwrap();
    }
    if name == "local" && args.len() == 2 {
        return normalize_lua_local(args);
    }
    if name == "assign" && args.len() == 2 {
        return normalize_lua_assign(args);
    }
    if name == "function" && args.len() == 2 {
        return normalize_lua_function(Symbol::new("anonymous"), args);
    }
    if name == "local-function" && args.len() == 3 {
        return normalize_lua_local_function(args);
    }
    if name == "table" {
        return normalize_lua_table(args);
    }
    let mut items = Vec::with_capacity(args.len() + 1);
    items.push(Expr::Symbol(Symbol::qualified("lua", name)));
    items.extend(args);
    Expr::List(items)
}

fn normalize_lua_local(args: Vec<Expr>) -> Expr {
    let mut args = args.into_iter();
    let bindings = args.next().unwrap();
    let values = args.next().unwrap();
    let names = binding_names(bindings);
    let mut items = Vec::new();
    items.push(Expr::Symbol(Symbol::qualified("lua", "local-values")));
    items.push(Expr::Vector(names));
    items.extend(vector_items(values));
    Expr::List(items)
}

fn normalize_lua_assign(args: Vec<Expr>) -> Expr {
    let mut args = args.into_iter();
    let targets = vector_items(args.next().unwrap());
    let values = vector_items(args.next().unwrap());
    if targets.len() == 1 && values.len() == 1 {
        return Expr::List(vec![
            Expr::Symbol(Symbol::qualified("lua", "assign")),
            targets.into_iter().next().unwrap(),
            values.into_iter().next().unwrap(),
        ]);
    }
    let mut items = Vec::new();
    items.push(Expr::Symbol(Symbol::qualified("lua", "block")));
    for (target, value) in targets.into_iter().zip(values) {
        items.push(Expr::List(vec![
            Expr::Symbol(Symbol::qualified("lua", "assign")),
            target,
            value,
        ]));
    }
    Expr::List(items)
}

fn normalize_lua_function(name: Symbol, args: Vec<Expr>) -> Expr {
    let mut args = args.into_iter();
    let params = args.next().unwrap();
    let body = args.next().unwrap();
    let (params, vararg) = split_params(params);
    Expr::List(vec![
        Expr::Symbol(Symbol::qualified("lua", "function")),
        Expr::Symbol(name),
        Expr::Vector(params),
        Expr::Bool(vararg),
        body,
    ])
}

fn normalize_lua_local_function(args: Vec<Expr>) -> Expr {
    let mut args = args.into_iter();
    let name = match args.next().unwrap() {
        Expr::Symbol(symbol) => symbol,
        _ => Symbol::new("anonymous"),
    };
    let function = normalize_lua_function(
        name.clone(),
        vec![args.next().unwrap(), args.next().unwrap()],
    );
    Expr::List(vec![
        Expr::Symbol(Symbol::qualified("lua", "local")),
        Expr::Symbol(name),
        function,
    ])
}

fn normalize_lua_table(args: Vec<Expr>) -> Expr {
    let mut items = vec![Expr::Symbol(Symbol::qualified("lua", "table"))];
    let mut next_index = 1;
    for field in args {
        if let Expr::List(parts) = field
            && let Some((Expr::Symbol(symbol), values)) = parts.split_first()
            && symbol.namespace.as_deref() == Some("lua")
        {
            match symbol.name.as_ref() {
                "field" if values.len() == 1 => {
                    items.push(lua_integer_expr(next_index));
                    items.push(values[0].clone());
                    next_index += 1;
                    continue;
                }
                "named-field" if values.len() == 2 => {
                    if let Expr::Symbol(key) = &values[0] {
                        items.push(Expr::String(key.name.to_string()));
                    } else {
                        items.push(values[0].clone());
                    }
                    items.push(values[1].clone());
                    continue;
                }
                "keyed-field" if values.len() == 2 => {
                    items.push(values[0].clone());
                    items.push(values[1].clone());
                    continue;
                }
                _ => {}
            }
        }
    }
    Expr::List(items)
}

fn binding_names(expr: Expr) -> Vec<Expr> {
    vector_items(expr)
        .into_iter()
        .map(|expr| match expr {
            Expr::List(items)
                if matches!(
                    items.first(),
                    Some(Expr::Symbol(symbol))
                        if symbol.namespace.as_deref() == Some("lua")
                            && symbol.name.as_ref() == "binding"
                ) =>
            {
                items.get(1).cloned().unwrap_or(Expr::Nil)
            }
            other => other,
        })
        .collect()
}

fn split_params(expr: Expr) -> (Vec<Expr>, bool) {
    let mut vararg = false;
    let params = vector_items(expr)
        .into_iter()
        .filter_map(|expr| {
            if matches!(
                &expr,
                Expr::List(items)
                    if matches!(
                        items.first(),
                        Some(Expr::Symbol(symbol))
                            if symbol.namespace.as_deref() == Some("lua")
                                && symbol.name.as_ref() == "vararg"
                    )
            ) {
                vararg = true;
                None
            } else {
                Some(expr)
            }
        })
        .collect();
    (params, vararg)
}

fn vector_items(expr: Expr) -> Vec<Expr> {
    match expr {
        Expr::Vector(items) | Expr::List(items) => items,
        other => vec![other],
    }
}

fn lua_integer_expr(value: i64) -> Expr {
    Expr::Number(NumberLiteral {
        domain: Symbol::qualified("lua", "number"),
        canonical: value.to_string(),
    })
}
